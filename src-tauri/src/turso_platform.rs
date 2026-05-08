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
    let client = reqwest::Client::builder()
        .user_agent(concat!("codo/", env!("CARGO_PKG_VERSION")))
        .build()?;

    // 1. Resolve the user's organization. The Platform API exposes the user's
    //    "personal" org by default; named orgs require an explicit slug.
    let org_slug = pick_org(&client, &req.platform_token).await?;

    // 2. Create the database (auto-suffix on name collision).
    let create_resp =
        create_database_with_retry(&client, &req.platform_token, &org_slug, &req.db_name).await?;
    let final_db_name = create_resp.name;
    // Turso's create-DB response carries the canonical `Hostname` for the
    // libSQL endpoint. Fall back to the org-slug-derived form if the API
    // didn't include it.
    let hostname = create_resp
        .hostname
        .unwrap_or_else(|| format!("{final_db_name}-{org_slug}.turso.io"));

    // 3. Mint a database-level auth token.
    let db_token = mint_db_token(&client, &req.platform_token, &org_slug, &final_db_name).await?;

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
    desired_name: &str,
) -> Result<CreatedDatabase> {
    match create_database(client, token, org_slug, desired_name).await {
        Ok(mut info) => {
            info.name = desired_name.to_string();
            Ok(info)
        }
        Err(first_err) => {
            // On the (rare) collision case, suffix and retry once.
            let suffix = chrono::Utc::now().timestamp_millis();
            let suffixed = format!("{desired_name}-{suffix}");
            match create_database(client, token, org_slug, &suffixed).await {
                Ok(mut info) => {
                    info.name = suffixed;
                    Ok(info)
                }
                Err(retry_err) => {
                    // Surface the original error since it's usually the more
                    // informative one (auth failure, quota, etc.).
                    Err(CodoError::internal(format!(
                        "create_database failed twice.\n\
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
    db_name: &str,
) -> Result<CreatedDatabase> {
    let resp = client
        .post(format!("{API_BASE}/organizations/{org_slug}/databases"))
        .bearer_auth(token)
        .header("Accept", "application/json")
        .json(&CreateDbBody {
            name: db_name,
            group: "default",
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
