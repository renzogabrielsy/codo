//! Persisted Turso DB credentials (URL + DB-level auth token).
//!
//! ## Where they live
//!
//! In a chmod-600 JSON file at `~/Library/Application Support/codo/
//! credentials.json` (macOS) / `%APPDATA%/codo/credentials.json` (Windows) /
//! `~/.local/share/codo/credentials.json` (Linux).
//!
//! ## Why not the OS Keychain
//!
//! PROJECT_BRAIN.md §3 + §5 #3 originally said "macOS Keychain via the
//! `keyring` crate." That changed in practice during Step 3 because the
//! Keychain rebinds to the binary's code signature: every Rust rebuild
//! during dev produces a binary with a different signature, so macOS
//! treats each rebuild as a different app and re-prompts the operator
//! to "Always Allow / Allow / Deny" on every read. With 5+ reads per
//! page reload that's tens of prompts an hour during active development.
//!
//! Crucially, an UNSIGNED Keychain entry doesn't actually defend against
//! "another app on this Mac reads your creds" any better than a chmod-600
//! file does — Keychain's "I trust this binary" mechanism keys on the
//! signed bundle identity, which we don't have. So switching to a file
//! is no real security loss in this configuration; it's a friction win.
//!
//! When codo gets a stable Apple Developer ID and signed bundle (brain
//! §5 #7), revisit and put the creds back in Keychain — at THAT point
//! the OS-level binding becomes meaningful.
//!
//! ## Auto-migration
//!
//! On first read after this change, codo also tries the legacy Keychain
//! entries. If found, it copies them into the file and deletes the
//! Keychain entries — one final pair of prompts, then the operator never
//! sees a prompt again until they manually rotate creds.

use crate::error::{CodoError, Result};
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

const KEYRING_SERVICE: &str = "codo";
const KEYRING_USER_DB_URL: &str = "turso_db_url";
const KEYRING_USER_DB_TOKEN: &str = "turso_db_token";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TursoCreds {
    pub url: String,
    pub token: String,
}

// ---------------------------------------------------------------------------
// File path
// ---------------------------------------------------------------------------

fn credentials_path() -> Result<PathBuf> {
    let base = match std::env::consts::OS {
        "macos" => {
            let home = std::env::var("HOME").map_err(|_| CodoError::internal("HOME unset"))?;
            PathBuf::from(home).join("Library/Application Support/codo")
        }
        "windows" => {
            let appdata =
                std::env::var("APPDATA").map_err(|_| CodoError::internal("APPDATA unset"))?;
            PathBuf::from(appdata).join("codo")
        }
        _ => {
            let home = std::env::var("HOME").map_err(|_| CodoError::internal("HOME unset"))?;
            PathBuf::from(home).join(".local/share/codo")
        }
    };
    Ok(base.join("credentials.json"))
}

// ---------------------------------------------------------------------------
// File-based store (no caching needed — the file IS the cache)
// ---------------------------------------------------------------------------

fn read_file() -> Result<Option<TursoCreds>> {
    let path = credentials_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path)?;
    let creds: TursoCreds = serde_json::from_slice(&bytes)?;
    Ok(Some(creds))
}

fn write_file(creds: &TursoCreds) -> Result<()> {
    let path = credentials_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(creds)?;
    std::fs::write(&path, bytes)?;
    set_owner_only_permissions(&path)?;
    Ok(())
}

fn delete_file() -> Result<()> {
    let path = credentials_path()?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &PathBuf) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}
#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &PathBuf) -> Result<()> {
    // On Windows the per-user AppData folder is already user-restricted by
    // default ACLs; nothing extra to do.
    Ok(())
}

// ---------------------------------------------------------------------------
// Legacy Keychain → file migration. Runs once when the file is empty but
// Keychain entries exist; produces one final pair of macOS prompts and then
// never touches Keychain again.
// ---------------------------------------------------------------------------

fn try_migrate_from_keychain() -> Result<Option<TursoCreds>> {
    let url = match keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER_DB_URL)?.get_password() {
        Ok(s) => s,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let token = match keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER_DB_TOKEN)?.get_password() {
        Ok(s) => s,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let creds = TursoCreds { url, token };
    write_file(&creds)?;
    // Best-effort cleanup. Failures here are non-fatal: the file is the new
    // truth either way.
    let _ = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER_DB_URL)
        .and_then(|e| e.delete_credential());
    let _ = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER_DB_TOKEN)
        .and_then(|e| e.delete_credential());
    tracing::info!("migrated Turso credentials from Keychain to credentials.json");
    Ok(Some(creds))
}

// ---------------------------------------------------------------------------
// Direct (no in-memory cache). With file storage there's no Keychain prompt
// to avoid, and `std::fs::read` of a ~200-byte JSON file is microseconds.
// ---------------------------------------------------------------------------

pub fn load() -> Result<Option<TursoCreds>> {
    if let Some(creds) = read_file()? {
        return Ok(Some(creds));
    }
    try_migrate_from_keychain()
}

pub fn store(creds: &TursoCreds) -> Result<()> {
    write_file(creds)
}

pub fn clear() -> Result<()> {
    delete_file()?;
    // Best-effort Keychain cleanup in case a stale entry survived migration.
    let _ = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER_DB_URL)
        .and_then(|e| e.delete_credential());
    let _ = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER_DB_TOKEN)
        .and_then(|e| e.delete_credential());
    Ok(())
}

pub fn require() -> Result<TursoCreds> {
    load()?.ok_or(CodoError::NotOnboarded)
}

// ---------------------------------------------------------------------------
// State-aware accessors. The in-memory cache is now mostly unnecessary (file
// reads are sub-millisecond) but kept so AppState's `creds` field is still
// the single read path other modules can use.
// ---------------------------------------------------------------------------

pub async fn load_via(state: &State<'_, AppState>) -> Result<Option<TursoCreds>> {
    {
        let cache = state.creds.lock().await;
        if cache.is_some() {
            return Ok(cache.clone());
        }
    }
    let from_disk = load()?;
    if let Some(creds) = &from_disk {
        let mut cache = state.creds.lock().await;
        *cache = Some(creds.clone());
    }
    Ok(from_disk)
}

pub async fn require_via(state: &State<'_, AppState>) -> Result<TursoCreds> {
    load_via(state).await?.ok_or(CodoError::NotOnboarded)
}

pub async fn store_via(state: &State<'_, AppState>, creds: &TursoCreds) -> Result<()> {
    store(creds)?;
    let mut cache = state.creds.lock().await;
    *cache = Some(creds.clone());
    Ok(())
}

#[allow(dead_code)]
pub async fn clear_via(state: &State<'_, AppState>) -> Result<()> {
    clear()?;
    let mut cache = state.creds.lock().await;
    *cache = None;
    Ok(())
}
