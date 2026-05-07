//! Persisted Turso DB credentials (URL + DB-level auth token) in the OS keyring.
//!
//! PROJECT_BRAIN.md §5 #3 / #12: the Turso URL and DB-level token are stored in
//! macOS Keychain / Windows Credential Manager / Linux Secret Service. The
//! Platform API token used during onboarding is never persisted — only the
//! per-database creds are.

use crate::error::{CodoError, Result};
use serde::{Deserialize, Serialize};

const SERVICE: &str = "codo";
const USER_DB_URL: &str = "turso_db_url";
const USER_DB_TOKEN: &str = "turso_db_token";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TursoCreds {
    pub url: String,
    pub token: String,
}

pub fn load() -> Result<Option<TursoCreds>> {
    let url = match keyring::Entry::new(SERVICE, USER_DB_URL)?.get_password() {
        Ok(s) => s,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let token = match keyring::Entry::new(SERVICE, USER_DB_TOKEN)?.get_password() {
        Ok(s) => s,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    Ok(Some(TursoCreds { url, token }))
}

pub fn store(creds: &TursoCreds) -> Result<()> {
    keyring::Entry::new(SERVICE, USER_DB_URL)?.set_password(&creds.url)?;
    keyring::Entry::new(SERVICE, USER_DB_TOKEN)?.set_password(&creds.token)?;
    Ok(())
}

pub fn clear() -> Result<()> {
    let _ = keyring::Entry::new(SERVICE, USER_DB_URL)?.delete_credential();
    let _ = keyring::Entry::new(SERVICE, USER_DB_TOKEN)?.delete_credential();
    Ok(())
}

pub fn require() -> Result<TursoCreds> {
    load()?.ok_or(CodoError::NotOnboarded)
}
