//! Turso Platform API client for first-launch onboarding.
//!
//! Implements PROJECT_BRAIN.md §5 #12:
//! - operator pastes ONE Platform API token
//! - codo creates a new database in their account
//! - codo mints a DB-level auth token
//! - codo persists `(db_url, db_token)` to the OS keyring
//! - codo discards the Platform API token immediately
//!
//! Failure modes the onboarding handles:
//! - invalid Platform token → re-prompt with helpful error
//! - DB name collision → auto-suffix and retry once
//! - org/group selection → prompt if multiple, default if one
//! - token expiry → only prompt for a new Platform token when actually needed

use crate::credentials::TursoCreds;
use crate::error::{CodoError, Result};
use serde::{Deserialize, Serialize};

const API_BASE: &str = "https://api.turso.tech/v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingRequest {
    pub platform_token: String,
    pub db_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingResult {
    pub creds: TursoCreds,
    pub org_slug: String,
    pub final_db_name: String,
}

/// Drive the full onboarding flow against the Turso Platform API. Returns the
/// DB-level URL + token (which the caller writes to the keyring), plus the
/// resolved org and database name for display.
pub async fn onboard(req: OnboardingRequest) -> Result<OnboardingResult> {
    // Aggressively normalize the token: strip ALL whitespace (including
    // non-breaking spaces and zero-width characters that can survive a
    // JS-side .trim() if the operator pasted from a styled webpage).
    let token = sanitize_token(&req.platform_token)?;

    let client = reqwest::Client::builder()
        .user_agent(concat!("codo/", env!("CARGO_PKG_VERSION")))
        .build()?;

    // 1. Resolve the user's organization. The Platform API exposes the user's
    //    "personal" org by default; named orgs require an explicit slug.
    let org_slug = pick_org(&client, &token).await?;

    // 2. Resolve the group to put the database in. New free-tier accounts
    //    don't always have a group called "default" — list what's there and
    //    pick one (creating "default" at a sensible location if none exist).
    let group = resolve_group(&client, &token, &org_slug).await?;

    // 3. Create the database (auto-suffix on name collision).
    let create_resp =
        create_database_with_retry(&client, &token, &org_slug, &group, &req.db_name).await?;
    let final_db_name = create_resp.name;
    // Turso's create-DB response carries the canonical `Hostname` for the
    // libSQL endpoint. Fall back to the org-slug-derived form if the API
    // didn't include it.
    let hostname = create_resp
        .hostname
        .unwrap_or_else(|| format!("{final_db_name}-{org_slug}.turso.io"));

    // 4. Mint a database-level auth token.
    let db_token = mint_db_token(&client, &token, &org_slug, &final_db_name).await?;

    let db_url = format!("libsql://{hostname}");

    Ok(OnboardingResult {
        creds: TursoCreds {
            url: db_url,
            token: db_token,
        },
        org_slug,
        final_db_name,
    })
}

// ---------------------------------------------------------------------------
// Token sanitization. JS-side `.trim()` only handles standard whitespace.
// Tokens copy-pasted from a styled webpage can carry non-breaking spaces,
// zero-width joiners, or other invisible characters that survive trim().
// Strip every Unicode whitespace + control char and verify the result still
// looks like a JWT (header.payload.signature).
// ---------------------------------------------------------------------------

fn sanitize_token(raw: &str) -> Result<String> {
    // Drop every whitespace + control char (NBSP \u{00A0}, ZWSP \u{200B},
    // BOM \u{FEFF}, regular spaces/newlines/tabs).
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect();

    if cleaned.is_empty() {
        return Err(CodoError::invalid("platform token is required"));
    }

    let segments: Vec<&str> = cleaned.split('.').collect();
    if segments.len() != 3 {
        return Err(CodoError::invalid(format!(
            "platform token doesn't look like a JWT — expected exactly 3 \
             dot-separated segments, got {}. Did you paste the wrong field, \
             or is there extra whitespace? (cleaned length: {} chars)",
            segments.len(),
            cleaned.len()
        )));
    }
    if !cleaned.starts_with("eyJ") {
        return Err(CodoError::invalid(
            "platform token doesn't look like a JWT — expected to start with \
             'eyJ'. Make sure you're copying from Account Settings → API \
             Tokens, and copying the whole token, not the token's name.",
        ));
    }

    Ok(cleaned)
}

// ---------------------------------------------------------------------------
// Robust JSON helpers — read the body as text first so that on a parse failure
// we can surface what the API actually returned. Without this, reqwest's
// `error decoding response body` is opaque.
// ---------------------------------------------------------------------------

async fn parse_json_body<T: for<'de> Deserialize<'de>>(
    label: &'static str,
    resp: reqwest::Response,
) -> Result<T> {
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(CodoError::TursoApi {
            status: status.as_u16(),
            body: text,
        });
    }
    serde_json::from_str::<T>(&text).map_err(|e| {
        CodoError::internal(format!(
            "Turso API ({label}) returned a body codo couldn't decode: {e}\n\
             status={status}\n\
             body={text}"
        ))
    })
}

#[derive(Debug, Deserialize)]
struct Organization {
    slug: String,
    #[serde(default, rename = "type")]
    org_type: String,
}

async fn pick_org(client: &reqwest::Client, token: &str) -> Result<String> {
    let resp = client
        .get(format!("{API_BASE}/organizations"))
        .bearer_auth(token)
        .header("Accept", "application/json")
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(CodoError::TursoApi {
            status: status.as_u16(),
            body: text,
        });
    }

    // Turso's API has shipped at least two response shapes here over time:
    //   - bare array:    [{"slug":"…","type":"personal"}, …]
    //   - wrapped:       {"organizations":[…]}
    // Accept either; surface the raw body if neither matches.
    let parsed: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        CodoError::internal(format!(
            "Turso /organizations returned non-JSON: {e}\n\
             status={status}\n\
             body={text}"
        ))
    })?;

    let orgs: Vec<Organization> = if parsed.is_array() {
        serde_json::from_value(parsed).map_err(|e| decode_err("orgs[bare]", e, &text))?
    } else if let Some(arr) = parsed.get("organizations") {
        serde_json::from_value(arr.clone()).map_err(|e| decode_err("orgs[wrapped]", e, &text))?
    } else {
        return Err(CodoError::internal(format!(
            "Turso /organizations returned an unrecognized shape — \
             expected an array or {{\"organizations\":[…]}}.\n\
             body={text}"
        )));
    };

    if orgs.is_empty() {
        return Err(CodoError::invalid(
            "no Turso organizations available for this token",
        ));
    }
    let chosen = orgs
        .iter()
        .find(|o| o.org_type == "personal")
        .or_else(|| orgs.first())
        .expect("non-empty list");
    Ok(chosen.slug.clone())
}

fn decode_err(label: &'static str, e: serde_json::Error, body: &str) -> CodoError {
    CodoError::internal(format!(
        "Turso decode failure ({label}): {e}\nbody={body}"
    ))
}

// ---------------------------------------------------------------------------
// Group resolution. Turso databases must live inside a "group" within an
// organization. The Free / Starter plan ships with one auto-created group on
// older accounts but newer signups don't always have one named "default" —
// hence the 400 'group not found' if we hardcode it.
//
// Strategy:
//   1. List groups for the org.
//   2. If any exist, prefer one named 'default', else use the first.
//   3. If none exist, create 'default' at a sensible location (`sin` is the
//      Singapore region, closest to Cebu; falls back to Turso's first
//      available location if 'sin' isn't enabled on this account).
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Group {
    name: String,
}

async fn resolve_group(
    client: &reqwest::Client,
    token: &str,
    org_slug: &str,
) -> Result<String> {
    // List existing groups.
    let resp = client
        .get(format!("{API_BASE}/organizations/{org_slug}/groups"))
        .bearer_auth(token)
        .header("Accept", "application/json")
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(CodoError::TursoApi {
            status: status.as_u16(),
            body: text,
        });
    }
    let parsed: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        CodoError::internal(format!(
            "Turso /groups returned non-JSON: {e}\nbody={text}"
        ))
    })?;
    let groups: Vec<Group> = if parsed.is_array() {
        serde_json::from_value(parsed).map_err(|e| decode_err("groups[bare]", e, &text))?
    } else if let Some(arr) = parsed.get("groups") {
        serde_json::from_value(arr.clone()).map_err(|e| decode_err("groups[wrapped]", e, &text))?
    } else {
        return Err(CodoError::internal(format!(
            "Turso /groups returned an unrecognized shape — \
             expected an array or {{\"groups\":[…]}}.\nbody={text}"
        )));
    };

    if !groups.is_empty() {
        let chosen = groups
            .iter()
            .find(|g| g.name == "default")
            .or_else(|| groups.first())
            .expect("non-empty");
        return Ok(chosen.name.clone());
    }

    // No groups — create one. Pick a location. `sin` (Singapore) is closest
    // to Cebu/Davao; if it's not allowed for this account we fall back to
    // whatever Turso's /locations endpoint says is available.
    let location = pick_location(client, token).await.unwrap_or_else(|_| "sin".to_string());
    create_group(client, token, org_slug, "default", &location).await?;
    Ok("default".to_string())
}

#[derive(Debug, Serialize)]
struct CreateGroupBody<'a> {
    name: &'a str,
    location: &'a str,
}

async fn create_group(
    client: &reqwest::Client,
    token: &str,
    org_slug: &str,
    name: &str,
    location: &str,
) -> Result<()> {
    let resp = client
        .post(format!("{API_BASE}/organizations/{org_slug}/groups"))
        .bearer_auth(token)
        .header("Accept", "application/json")
        .json(&CreateGroupBody { name, location })
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(CodoError::TursoApi {
            status: status.as_u16(),
            body: format!("{text} (creating group '{name}' at '{location}')"),
        });
    }
    Ok(())
}

async fn pick_location(client: &reqwest::Client, token: &str) -> Result<String> {
    let resp = client
        .get(format!("{API_BASE}/locations"))
        .bearer_auth(token)
        .header("Accept", "application/json")
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(CodoError::invalid("could not list Turso locations"));
    }
    let text = resp.text().await.unwrap_or_default();
    let parsed: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| CodoError::internal(format!("Turso /locations non-JSON: {e}\n{text}")))?;
    // /locations returns { "locations": { "sin": "Singapore", "lax": "...", ... } }
    // Prefer 'sin' (closest to Cebu/Davao), fallback to whatever's first.
    if let Some(map) = parsed.get("locations").and_then(|v| v.as_object()) {
        if map.contains_key("sin") {
            return Ok("sin".to_string());
        }
        if let Some(first_key) = map.keys().next() {
            return Ok(first_key.clone());
        }
    }
    Err(CodoError::invalid("no Turso locations available"))
}

#[derive(Debug, Serialize)]
struct CreateDbBody<'a> {
    name: &'a str,
    group: &'a str,
}

#[derive(Debug, Default)]
struct CreatedDatabase {
    name: String,
    hostname: Option<String>,
}

async fn create_database_with_retry(
    client: &reqwest::Client,
    token: &str,
    org_slug: &str,
    group: &str,
    desired_name: &str,
) -> Result<CreatedDatabase> {
    match create_database(client, token, org_slug, group, desired_name).await {
        Ok(mut info) => {
            info.name = desired_name.to_string();
            Ok(info)
        }
        Err(first_err) => {
            // On the (rare) collision case, suffix and retry once.
            let suffix = chrono::Utc::now().timestamp_millis();
            let suffixed = format!("{desired_name}-{suffix}");
            match create_database(client, token, org_slug, group, &suffixed).await {
                Ok(mut info) => {
                    info.name = suffixed;
                    Ok(info)
                }
                Err(retry_err) => {
                    // Surface the original error since it's usually the more
                    // informative one (auth failure, quota, etc.).
                    Err(CodoError::internal(format!(
                        "create_database failed twice (group={group}).\n\
                         first attempt ({desired_name}): {first_err}\n\
                         retry ({suffixed}): {retry_err}"
                    )))
                }
            }
        }
    }
}

async fn create_database(
    client: &reqwest::Client,
    token: &str,
    org_slug: &str,
    group: &str,
    db_name: &str,
) -> Result<CreatedDatabase> {
    let resp = client
        .post(format!("{API_BASE}/organizations/{org_slug}/databases"))
        .bearer_auth(token)
        .header("Accept", "application/json")
        .json(&CreateDbBody {
            name: db_name,
            group,
        })
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(CodoError::TursoApi {
            status: status.as_u16(),
            body: text,
        });
    }

    // Try to extract Hostname from a few known response shapes:
    //   {"database":{"Hostname":"…","Name":"…", …}}      (older docs)
    //   {"Hostname":"…","Name":"…"}                       (flat)
    //   {"database":{"hostname":"…","name":"…"}}          (lowercase variant)
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    let inner = v.get("database").unwrap_or(&v);
    let hostname = inner
        .get("Hostname")
        .or_else(|| inner.get("hostname"))
        .and_then(|h| h.as_str())
        .map(|s| s.to_string());
    Ok(CreatedDatabase {
        name: db_name.to_string(),
        hostname,
    })
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    jwt: String,
}

async fn mint_db_token(
    client: &reqwest::Client,
    platform_token: &str,
    org_slug: &str,
    db_name: &str,
) -> Result<String> {
    let url = format!(
        "{API_BASE}/organizations/{org_slug}/databases/{db_name}/auth/tokens?authorization=full-access"
    );
    let resp = client
        .post(url)
        .bearer_auth(platform_token)
        .header("Accept", "application/json")
        .send()
        .await?;
    let tok: TokenResponse = parse_json_body("mint_db_token", resp).await?;
    Ok(tok.jwt)
}
