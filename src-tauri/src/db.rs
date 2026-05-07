//! Database connection + remote-first migration runner.
//!
//! PROJECT_BRAIN.md §5 #1: schema migrations apply to the REMOTE Turso DB
//! first; the local libSQL embedded replica picks them up on the next
//! `db.sync()`. DDL applied only locally would be silently overwritten on the
//! next pull.

use crate::credentials::TursoCreds;
use crate::error::{CodoError, Result};
use libsql::{params, Builder, Connection, Database};
use std::path::PathBuf;

/// Embedded migrations. Append-only per PROJECT_BRAIN.md §8 pattern #1 — never
/// edit a previous migration; always add a new file and a new entry here.
pub const MIGRATIONS: &[(i64, &str, &str)] = &[
    (1, "v1_initial", include_str!("../../migrations/v1.sql")),
];

/// Lookup-table seed values. Idempotent (`INSERT OR IGNORE`).
pub const SEED_SQL: &str = include_str!("../../migrations/seed.sql");

/// Apply every pending migration to a target `Connection`. Used both against
/// the remote DB during onboarding and against the local replica defensively
/// (so that `cargo test` can run without a Turso roundtrip).
pub async fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute(
        r#"CREATE TABLE IF NOT EXISTS schema_version (
            version    INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        )"#,
        params![],
    )
    .await?;

    let mut applied = std::collections::HashSet::<i64>::new();
    let mut rows = conn
        .query("SELECT version FROM schema_version", params![])
        .await?;
    while let Some(row) = rows.next().await? {
        let v: i64 = row.get(0)?;
        applied.insert(v);
    }

    for &(version, name, sql) in MIGRATIONS {
        if applied.contains(&version) {
            tracing::debug!(version, name, "migration already applied");
            continue;
        }
        tracing::info!(version, name, "applying migration");
        execute_batch(conn, sql).await?;
        conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (?, ?)",
            params![version, chrono::Utc::now().to_rfc3339()],
        )
        .await?;
    }

    // Seed lookup tables (idempotent).
    execute_batch(conn, SEED_SQL).await?;

    Ok(())
}

/// libSQL doesn't ship a helper for multi-statement scripts, so we split on
/// `;\n`-ish boundaries while respecting the simple statements our SQL files
/// use. The schema and seed files are hand-written and don't contain
/// semicolons inside string literals, so naive splitting is safe.
async fn execute_batch(conn: &Connection, script: &str) -> Result<()> {
    let mut buf = String::new();
    for line in script.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("--") || trimmed.is_empty() {
            // strip line comments + blank lines
            continue;
        }
        buf.push_str(line);
        buf.push('\n');
        // Statement boundary heuristic: a line ending in `;` outside quotes.
        if line.trim_end().ends_with(';') {
            let stmt = buf.trim();
            if !stmt.is_empty() {
                conn.execute(stmt, params![]).await.map_err(|e| {
                    CodoError::Internal(format!("migration statement failed:\n{stmt}\n\n{e}"))
                })?;
            }
            buf.clear();
        }
    }
    let tail = buf.trim();
    if !tail.is_empty() {
        conn.execute(tail, params![])
            .await
            .map_err(|e| CodoError::Internal(format!("migration tail failed:\n{tail}\n\n{e}")))?;
    }
    Ok(())
}

/// Apply migrations DIRECTLY to the remote Turso DB. Per PROJECT_BRAIN.md §5
/// #1 this must happen before the local replica is built — otherwise the next
/// `db.sync()` would clobber the local DDL.
pub async fn apply_to_remote(creds: &TursoCreds) -> Result<()> {
    let db = Builder::new_remote(creds.url.clone(), creds.token.clone())
        .build()
        .await?;
    let conn = db.connect()?;
    run_migrations(&conn).await
}

/// Build a local libSQL embedded replica that auto-syncs to Turso. Returns the
/// `Database` (so callers can hold it for `sync()`) and a freshly-opened
/// `Connection`.
pub async fn open_local_replica(creds: &TursoCreds) -> Result<(Database, Connection)> {
    let path = local_replica_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let db = Builder::new_remote_replica(path, creds.url.clone(), creds.token.clone())
        .build()
        .await?;
    db.sync().await?;
    let conn = db.connect()?;
    Ok((db, conn))
}

/// Local replica path under the OS app-data dir.
pub fn local_replica_path() -> Result<PathBuf> {
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
    Ok(base.join("codo.db"))
}

// Integration tests in `tests/validity_matrix.rs` build their own tmp DB
// directly via `libsql::Builder::new_local` + `run_migrations`. We don't
// re-export a helper here to avoid pulling tempfile into the runtime
// dependency surface.
