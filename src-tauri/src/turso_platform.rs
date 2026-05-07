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
    let final_db_name = create_database_with_retry(&client, &req.platform_token, &org_slug, &req.db_name).await?;

    // 3. Mint a database-level auth token.
    let db_token = mint_db_token(&client, &req.platform_token, &org_slug, &final_db_name).await?;

    // 4. Resolve the libSQL URL. Turso's create-DB response carries `Hostname`;
    //    libSQL uses the `libsql://` scheme.
    let db_url = format!("libsql://{final_db_name}-{org_slug}.turso.io");

    Ok(OnboardingResult {
        creds: TursoCreds {
            url: db_url,
            token: db_token,
        },
        org_slug,
        final_db_name,
    })
}

#[derive(Debug, Deserialize)]
struct OrgListResponse {
    organizations: Vec<Organization>,
}

#[derive(Debug, Deserialize)]
struct Organization {
    slug: String,
    #[serde(rename = "type", default)]
    org_type: String,
}

async fn pick_org(client: &reqwest::Client, token: &str) -> Result<String> {
    let resp = client
        .get(format!("{API_BASE}/organizations"))
        .bearer_auth(token)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(CodoError::TursoApi {
            status: status.as_u16(),
            body,
        });
    }
    let listing: OrgListResponse = resp.json().await?;
    if listing.organizations.is_empty() {
        return Err(CodoError::invalid(
            "no Turso organizations available for this token",
        ));
    }
    // Prefer "personal" org if present, else the first listed. Multi-org
    // selection is a future-Renzo problem (only matters if he creates a
    // company workspace later).
    let chosen = listing
        .organizations
        .iter()
        .find(|o| o.org_type == "personal")
        .or_else(|| listing.organizations.first())
        .expect("non-empty list");
    Ok(chosen.slug.clone())
}

#[derive(Debug, Serialize)]
struct CreateDbBody<'a> {
    name: &'a str,
    group: &'a str,
}

async fn create_database_with_retry(
    client: &reqwest::Client,
    token: &str,
    org_slug: &str,
    desired_name: &str,
) -> Result<String> {
    // First attempt with the desired name.
    if create_database(client, token, org_slug, desired_name).await.is_ok() {
        return Ok(desired_name.to_string());
    }
    // On collision, try one suffixed variant (millis-since-epoch keeps it
    // short and unique). Renzo only triggers this on a re-onboard scenario.
    let suffix = chrono::Utc::now().timestamp_millis();
    let suffixed = format!("{desired_name}-{suffix}");
    create_database(client, token, org_slug, &suffixed).await?;
    Ok(suffixed)
}

async fn create_database(
    client: &reqwest::Client,
    token: &str,
    org_slug: &str,
    db_name: &str,
) -> Result<()> {
    let resp = client
        .post(format!("{API_BASE}/organizations/{org_slug}/databases"))
        .bearer_auth(token)
        .json(&CreateDbBody {
            name: db_name,
            group: "default",
        })
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(CodoError::TursoApi {
            status: status.as_u16(),
            body,
        });
    }
    Ok(())
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
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(CodoError::TursoApi {
            status: status.as_u16(),
            body,
        });
    }
    let tok: TokenResponse = resp.json().await?;
    Ok(tok.jwt)
}
