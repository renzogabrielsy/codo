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

use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared mutable app state. The libSQL connection is built lazily after
/// onboarding lands credentials in the keyring.
#[derive(Default)]
pub struct AppState {
    pub conn: Arc<Mutex<Option<libsql::Connection>>>,
    pub db: Arc<Mutex<Option<libsql::Database>>>,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running codo");
}
