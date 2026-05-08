// codo — Tauri v2 + Rust + SvelteKit + libSQL desktop app.
// PROJECT_BRAIN.md is the single source of truth for everything in this crate.

pub mod canonicalize;
pub mod commands;
pub mod credentials;
pub mod db;
pub mod direction;
pub mod error;
pub mod ledger;
pub mod turso_platform;
pub mod validation;

use crate::credentials::TursoCreds;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared mutable app state.
///
/// `local_db` is the **primary** store — a local SQLite file at
/// `~/Library/Application Support/codo/codo.db`. Every Tauri command reads
/// and writes here. `db.connect()` per-command is cheap (no network).
///
/// `remote_db` is the cloud **backup** target — the existing Turso DB. Lazy:
/// only built when the Sync flow needs it. Single-direction push.
///
/// `creds` caches the chmod-600 credentials file read.
///
/// We do NOT cache `Connection` — fresh-conn-per-command sidesteps stale
/// streams (the Hrana 'stream not found' bug Renzo hit in direct-remote mode).
#[derive(Default)]
pub struct AppState {
    pub local_db: Arc<Mutex<Option<libsql::Database>>>,
    pub remote_db: Arc<Mutex<Option<libsql::Database>>>,
    pub creds: Arc<Mutex<Option<TursoCreds>>>,
}

/// Tauri entry point. Called from `main.rs`.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("codo_lib=info,warn")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::is_onboarded,
            commands::onboard_turso,
            commands::run_remote_migrations,
            commands::init_db,
            commands::list_warehouses,
            commands::current_version,
            commands::list_lookups,
            commands::create_production_event,
            commands::create_production_events_bulk,
            commands::list_recent_events,
            commands::sync_now,
            commands::get_sync_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running codo");
}
