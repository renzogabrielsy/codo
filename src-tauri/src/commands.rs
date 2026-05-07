//! Tauri command surface.
//!
//! This is the JS↔Rust bridge. Step 2 ships the minimum to demonstrate the
//! boot path (onboarding → migrations → local replica → Hello). Step 3 will
//! add `create_production_event` and friends.

use crate::credentials::{self, TursoCreds};
use crate::db;
use crate::error::{CodoError, Result};
use crate::turso_platform::{self, OnboardingRequest};
use crate::AppState;
use libsql::params;
use serde::Serialize;
use tauri::State;

/// True if Turso credentials are already in the keyring (i.e. onboarding has
/// happened on this machine before).
#[tauri::command]
pub async fn is_onboarded() -> Result<bool> {
    Ok(credentials::load()?.is_some())
}

/// First-launch onboarding. Takes a Turso Platform API token, creates a DB,
/// mints a DB-level token, persists the URL + token to the OS keyring, and
/// throws the Platform token away. PROJECT_BRAIN.md §5 #12.
#[tauri::command]
pub async fn onboard_turso(
    platform_token: String,
    db_name: String,
) -> Result<OnboardSummary> {
    if platform_token.trim().is_empty() {
        return Err(CodoError::invalid("platform token is required"));
    }
    let req = OnboardingRequest {
        platform_token,
        db_name: if db_name.trim().is_empty() {
            "codo".to_string()
        } else {
            db_name
        },
    };
    let result = turso_platform::onboard(req).await?;
    credentials::store(&result.creds)?;
    Ok(OnboardSummary {
        org_slug: result.org_slug,
        db_name: result.final_db_name,
        db_url: result.creds.url,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct OnboardSummary {
    pub org_slug: String,
    pub db_name: String,
    pub db_url: String,
}

/// Apply DDL migrations to the REMOTE Turso DB. Per PROJECT_BRAIN.md §5 #1
/// this MUST happen before the local libSQL replica is opened — otherwise
/// `db.sync()` would clobber the local DDL.
#[tauri::command]
pub async fn run_remote_migrations() -> Result<()> {
    let creds = credentials::require()?;
    db::apply_to_remote(&creds).await?;
    Ok(())
}

/// Open a local libSQL embedded replica that auto-syncs to the remote Turso
/// DB. Stores the connection in `AppState` for subsequent commands.
#[tauri::command]
pub async fn init_local_replica(state: State<'_, AppState>) -> Result<()> {
    let creds: TursoCreds = credentials::require()?;
    let (database, conn) = db::open_local_replica(&creds).await?;
    // Defensive: also run migrations against the local replica in case the
    // remote sync hasn't completed for some reason. Idempotent.
    db::run_migrations(&conn).await?;

    let mut conn_guard = state.conn.lock().await;
    *conn_guard = Some(conn);
    let mut db_guard = state.db.lock().await;
    *db_guard = Some(database);
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct WarehouseRow {
    pub id: i64,
    pub code: String,
    pub default_unit: String,
}

/// Tiny demo command for the "Hello" page — proves the local replica is
/// connected and the lookup tables seeded. Listed in WHSE 1, 2, 3, 5, 7 order.
#[tauri::command]
pub async fn list_warehouses(state: State<'_, AppState>) -> Result<Vec<WarehouseRow>> {
    let guard = state.conn.lock().await;
    let conn = guard
        .as_ref()
        .ok_or_else(|| CodoError::internal("local replica not initialized"))?;

    let mut rows = conn
        .query(
            "SELECT id, code, default_unit FROM warehouse ORDER BY id",
            params![],
        )
        .await?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        out.push(WarehouseRow {
            id: row.get(0)?,
            code: row.get(1)?,
            default_unit: row.get(2)?,
        });
    }
    Ok(out)
}
