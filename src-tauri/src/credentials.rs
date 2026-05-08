//! Persisted Turso DB credentials (URL + DB-level auth token) in the OS keyring.
//!
//! PROJECT_BRAIN.md §5 #3 / #12: the Turso URL and DB-level token are stored in
//! macOS Keychain / Windows Credential Manager / Linux Secret Service. The
//! Platform API token used during onboarding is never persisted — only the
//! per-database creds are.
//!
//! ## Caching matters more than you'd think
//!
//! On macOS, every Keychain read against an unsigned binary triggers an
//! "always allow" / "allow" / "deny" prompt — because each rebuild of the
//! dev binary has a different code signature, macOS treats it as a different
//! app. Without caching, the boot flow would prompt 5+ times per page
//! reload (is_onboarded → run_remote_migrations → init_local_replica each
//! reads URL + token).
//!
//! `load_via(state)` and `require_via(state)` read the cache in `AppState`
//! first and only hit the Keychain if the cache is empty. After the first
//! successful read of a session, every subsequent command is a fast in-
//! memory lookup. `clear()` wipes both Keychain and cache (used when the
//! operator wants to re-onboard).

use crate::error::{CodoError, Result};
use crate::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

const SERVICE: &str = "codo";
const USER_DB_URL: &str = "turso_db_url";
const USER_DB_TOKEN: &str = "turso_db_token";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TursoCreds {
    pub url: String,
    pub token: String,
}

// ---------------------------------------------------------------------------
// Direct Keychain access (no caching). Use these only at boot or when
// rotating creds. Every call may trigger a macOS auth prompt on an unsigned
// dev binary — wrap in `load_via` / `require_via` for the cache.
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Cache-aware accessors. Prefer these everywhere a Tauri command needs creds.
// ---------------------------------------------------------------------------

/// Cache-aware load: returns `Some` if creds are cached, falls through to
/// Keychain otherwise (and populates the cache on success).
pub async fn load_via(state: &State<'_, AppState>) -> Result<Option<TursoCreds>> {
    {
        let cache = state.creds.lock().await;
        if cache.is_some() {
            return Ok(cache.clone());
        }
    }
    let from_keyring = load()?;
    if let Some(creds) = &from_keyring {
        let mut cache = state.creds.lock().await;
        *cache = Some(creds.clone());
    }
    Ok(from_keyring)
}

/// Like `require()` but cache-aware. Use everywhere a command needs creds.
pub async fn require_via(state: &State<'_, AppState>) -> Result<TursoCreds> {
    load_via(state).await?.ok_or(CodoError::NotOnboarded)
}

/// Persist new creds AND update the cache.
pub async fn store_via(state: &State<'_, AppState>, creds: &TursoCreds) -> Result<()> {
    store(creds)?;
    let mut cache = state.creds.lock().await;
    *cache = Some(creds.clone());
    Ok(())
}

/// Wipe both Keychain and cache.
#[allow(dead_code)]
pub async fn clear_via(state: &State<'_, AppState>) -> Result<()> {
    clear()?;
    let mut cache = state.creds.lock().await;
    *cache = None;
    Ok(())
}
