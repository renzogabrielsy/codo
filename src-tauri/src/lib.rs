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

/// Shared mutable app state. Holds the libSQL connection AND a cached copy
/// of the Turso credentials — see `credentials.rs` for why caching matters
/// (macOS Keychain prompts every read on unsigned dev binaries).
#[derive(Default)]
pub struct AppState {
    pub conn: Arc<Mutex<Option<libsql::Connection>>>,
    pub db: Arc<Mutex<Option<libsql::Database>>>,
    /// Cached after the first successful Keychain read of the session.
    /// Cleared by `credentials::clear()` if the operator ever wants to rotate.
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
            commands::init_local_replica,
            commands::list_warehouses,
            commands::current_version,
            commands::list_lookups,
            commands::create_production_event,
            commands::create_production_events_bulk,
            commands::list_recent_events,
        ])
        .run(tauri::generate_context!())
        .expect("error while running codo");
}
