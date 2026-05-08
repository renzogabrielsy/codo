//! Database layer.
//!
//! ## Architecture (single-user, local-first, manual cloud backup)
//!
//! - **Primary store: a local SQLite file** at `~/Library/Application Support/
//!   codo/codo.db` (macOS), opened via `libsql::Builder::new_local`. This is
//!   plain SQLite — the most-deployed DB engine on the planet — and is what
//!   every Tauri command reads from and writes to.
//!
//! - **Cloud backup target: the existing Turso DB**. Used only by the manual
//!   "Sync now" path (`sync_local_to_remote`). One-way push: rows where
//!   `dirty = 1` get INSERT-OR-IGNOREd into the remote, then `dirty` is
//!   cleared on the local copy. The remote is a safety net, not a sync
//!   counterpart.
//!
//! - **Bootstrap** on first launch after this swap: if the local DB has zero
//!   `production_event` rows but the remote has some (because Renzo's been
//!   testing onboarding against direct-remote), pull them down once.
//!
//! ## Why this architecture
//!
//! Three previous attempts:
//!   1. libsql 0.6 embedded replica → Turso retired the protocol server-side
//!      ('deprecated version of sync' handshake error).
//!   2. libsql 0.9 embedded replica → same protocol, same error.
//!   3. libsql 0.9 direct-remote → works but breaks every time wifi drops, and
//!      Hrana streams idle-time-out (the 'stream not found' bug Renzo hit
//!      after ~1.5h of idle).
//!
//! Considered alternatives:
//!   - The new `turso` crate's offline-sync mode → still public beta, "data
//!     loss possible" per their own docs. Stability > shiny.
//!   - Build our own bidirectional sync layer on plain SQLite → brain §3
//!     warned against this; high bug surface for a single-user app that
//!     doesn't need real conflict resolution.
//!
//! Single-user + manual backup sidesteps every conflict-resolution hazard,
//! works perfectly offline, uses the most stable possible storage engine,
//! AND keeps the existing Turso DB usable as a remote read endpoint for the
//! eventual management dashboard. Brain §3 + §4.2 updated to reflect.

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

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

pub fn app_data_dir() -> Result<PathBuf> {
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
    Ok(base)
}

pub fn local_db_path() -> Result<PathBuf> {
    Ok(app_data_dir()?.join("codo.db"))
}

// ---------------------------------------------------------------------------
// Migration runner — works against any libsql Connection (local or remote).
// ---------------------------------------------------------------------------

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

    execute_batch(conn, SEED_SQL).await?;
    Ok(())
}

async fn execute_batch(conn: &Connection, script: &str) -> Result<()> {
    let mut buf = String::new();
    for line in script.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("--") || trimmed.is_empty() {
            continue;
        }
        buf.push_str(line);
        buf.push('\n');
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

// ---------------------------------------------------------------------------
// Local primary store
// ---------------------------------------------------------------------------

/// Open (creating if needed) the local SQLite primary DB. Runs schema
/// migrations + seeds idempotently. Returns the `Database` so callers can
/// open fresh per-command Connections via `db.connect()`.
pub async fn open_local_db() -> Result<Database> {
    let path = local_db_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db = Builder::new_local(&path).build().await?;
    let conn = db.connect()?;
    run_migrations(&conn).await?;
    drop(conn);
    Ok(db)
}

// ---------------------------------------------------------------------------
// Remote backup target
// ---------------------------------------------------------------------------

/// Open the remote Turso DB as the backup target. Migrations are run on
/// demand from the manual sync path, not here, so this is a cheap
/// builder-only operation.
pub async fn open_remote_db(creds: &TursoCreds) -> Result<Database> {
    Ok(Builder::new_remote(creds.url.clone(), creds.token.clone())
        .build()
        .await?)
}

/// Apply schema migrations against the REMOTE DB. Used during onboarding
/// (so the remote schema matches local from day 1) and from the manual
/// sync path (idempotent — no-op if remote is already current).
pub async fn apply_to_remote(creds: &TursoCreds) -> Result<()> {
    let db = open_remote_db(creds).await?;
    let conn = db.connect()?;
    run_migrations(&conn).await
}

// ---------------------------------------------------------------------------
// Bootstrap: pull existing remote rows into a fresh local DB.
// ---------------------------------------------------------------------------

/// If the local `production_event` table is empty AND the remote has rows,
/// copy them down. Idempotent: subsequent calls find local non-empty and
/// skip. Returns the count of rows pulled (0 if nothing to do).
pub async fn bootstrap_local_from_remote(
    local: &Connection,
    remote: &Connection,
) -> Result<u64> {
    // Cheap zero-row check — no full scan.
    let mut local_count_rows = local
        .query("SELECT COUNT(*) FROM production_event", params![])
        .await?;
    let local_count: i64 = local_count_rows
        .next()
        .await?
        .ok_or_else(|| CodoError::internal("local count returned no rows"))?
        .get(0)?;
    if local_count > 0 {
        return Ok(0);
    }

    // Copy rows table-by-table. We only mirror tables that hold operator-
    // written data; lookup tables are seeded deterministically locally.
    let mut total: u64 = 0;
    total += copy_production_events(local, remote).await?;
    total += copy_dvo_batches(local, remote).await?;
    total += copy_dvo_receipts(local, remote).await?;
    total += copy_warehouse_opening_balances(local, remote).await?;
    total += copy_drift_log(local, remote).await?;
    Ok(total)
}

async fn copy_production_events(local: &Connection, remote: &Connection) -> Result<u64> {
    let mut rows = remote
        .query(
            "SELECT id, recv_date, prod_date, batch,
                    shift_id, grade_id, plant_id, warehouse_id, source_location_id,
                    weight_kg, disposition_kind, partner_equipment_id,
                    flec_count, whse_side, flec_stat, dvo_batch_id,
                    unique_tag, notes, dirty, created_at, updated_at
             FROM production_event",
            params![],
        )
        .await?;
    let mut copied: u64 = 0;
    while let Some(r) = rows.next().await? {
        let id: i64 = r.get(0)?;
        let recv_date: String = r.get(1)?;
        let prod_date: Option<String> = r.get(2).ok();
        let batch: String = r.get(3)?;
        let shift_id: Option<i64> = r.get(4).ok();
        let grade_id: i64 = r.get(5)?;
        let plant_id: Option<i64> = r.get(6).ok();
        let warehouse_id: Option<i64> = r.get(7).ok();
        let source_location_id: i64 = r.get(8)?;
        let weight_kg: f64 = r.get(9)?;
        let disposition_kind: String = r.get(10)?;
        let partner_equipment_id: Option<i64> = r.get(11).ok();
        let flec_count: Option<i64> = r.get(12).ok();
        let whse_side: Option<String> = r.get(13).ok();
        let flec_stat: Option<String> = r.get(14).ok();
        let dvo_batch_id: Option<i64> = r.get(15).ok();
        let unique_tag: String = r.get(16)?;
        let notes: Option<String> = r.get(17).ok();
        let _dirty: i64 = r.get(18)?;
        let created_at: String = r.get(19)?;
        let updated_at: String = r.get(20)?;

        let n = local
            .execute(
                "INSERT OR IGNORE INTO production_event (
                    id, recv_date, prod_date, batch,
                    shift_id, grade_id, plant_id, warehouse_id, source_location_id,
                    weight_kg, disposition_kind, partner_equipment_id,
                    flec_count, whse_side, flec_stat, dvo_batch_id,
                    unique_tag, notes, dirty, created_at, updated_at
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
                // dirty=0 because the row is already in the remote.
                params![
                    id,
                    recv_date,
                    prod_date,
                    batch,
                    shift_id,
                    grade_id,
                    plant_id,
                    warehouse_id,
                    source_location_id,
                    weight_kg,
                    disposition_kind,
                    partner_equipment_id,
                    flec_count,
                    whse_side,
                    flec_stat,
                    dvo_batch_id,
                    unique_tag,
                    notes,
                    created_at,
                    updated_at,
                ],
            )
            .await?;
        copied += n;
    }
    Ok(copied)
}

async fn copy_dvo_batches(local: &Connection, remote: &Connection) -> Result<u64> {
    let mut rows = remote.query("SELECT id, code, warehouse_id, start_month, year, side, status, opened_at, closed_at, closed_by, notes, frozen_transit_loss, frozen_yield_loss, created_at, updated_at FROM dvo_batch", params![]).await?;
    let mut copied: u64 = 0;
    while let Some(r) = rows.next().await? {
        let id: i64 = r.get(0)?;
        let code: String = r.get(1)?;
        let warehouse_id: i64 = r.get(2)?;
        let start_month: i64 = r.get(3)?;
        let year: i64 = r.get(4)?;
        let side: String = r.get(5)?;
        let status: String = r.get(6)?;
        let opened_at: String = r.get(7)?;
        let closed_at: Option<String> = r.get(8).ok();
        let closed_by: Option<String> = r.get(9).ok();
        let notes: Option<String> = r.get(10).ok();
        let frozen_transit_loss: Option<f64> = r.get(11).ok();
        let frozen_yield_loss: Option<f64> = r.get(12).ok();
        let created_at: String = r.get(13)?;
        let updated_at: String = r.get(14)?;
        let n = local
            .execute(
                "INSERT OR IGNORE INTO dvo_batch (id, code, warehouse_id, start_month, year, side, status, opened_at, closed_at, closed_by, notes, frozen_transit_loss, frozen_yield_loss, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![id, code, warehouse_id, start_month, year, side, status, opened_at, closed_at, closed_by, notes, frozen_transit_loss, frozen_yield_loss, created_at, updated_at],
            )
            .await?;
        copied += n;
    }
    Ok(copied)
}

async fn copy_dvo_receipts(local: &Connection, remote: &Connection) -> Result<u64> {
    let mut rows = remote.query("SELECT id, dvo_batch_id, recv_date, gothong_slip, sack_count, dvo_declared_weight_kg, cebu_declared_weight_kg, at_cebu_cy, notes, created_at, updated_at FROM dvo_receipt", params![]).await?;
    let mut copied: u64 = 0;
    while let Some(r) = rows.next().await? {
        let id: i64 = r.get(0)?;
        let dvo_batch_id: i64 = r.get(1)?;
        let recv_date: String = r.get(2)?;
        let gothong_slip: Option<String> = r.get(3).ok();
        let sack_count: Option<i64> = r.get(4).ok();
        let dvo_declared: f64 = r.get(5)?;
        let cebu_declared: f64 = r.get(6)?;
        let at_cebu_cy: i64 = r.get(7)?;
        let notes: Option<String> = r.get(8).ok();
        let created_at: String = r.get(9)?;
        let updated_at: String = r.get(10)?;
        let n = local
            .execute(
                "INSERT OR IGNORE INTO dvo_receipt (id, dvo_batch_id, recv_date, gothong_slip, sack_count, dvo_declared_weight_kg, cebu_declared_weight_kg, at_cebu_cy, notes, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![id, dvo_batch_id, recv_date, gothong_slip, sack_count, dvo_declared, cebu_declared, at_cebu_cy, notes, created_at, updated_at],
            )
            .await?;
        copied += n;
    }
    Ok(copied)
}

async fn copy_warehouse_opening_balances(local: &Connection, remote: &Connection) -> Result<u64> {
    let mut rows = remote.query("SELECT id, warehouse_id, grade_id, side, period_start_date, opening_flec_count, notes, created_at, updated_at FROM warehouse_opening_balance", params![]).await?;
    let mut copied: u64 = 0;
    while let Some(r) = rows.next().await? {
        let id: i64 = r.get(0)?;
        let warehouse_id: i64 = r.get(1)?;
        let grade_id: i64 = r.get(2)?;
        let side: String = r.get(3)?;
        let period_start_date: String = r.get(4)?;
        let opening_flec_count: i64 = r.get(5)?;
        let notes: Option<String> = r.get(6).ok();
        let created_at: String = r.get(7)?;
        let updated_at: String = r.get(8)?;
        let n = local
            .execute(
                "INSERT OR IGNORE INTO warehouse_opening_balance (id, warehouse_id, grade_id, side, period_start_date, opening_flec_count, notes, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![id, warehouse_id, grade_id, side, period_start_date, opening_flec_count, notes, created_at, updated_at],
            )
            .await?;
        copied += n;
    }
    Ok(copied)
}

async fn copy_drift_log(local: &Connection, remote: &Connection) -> Result<u64> {
    let mut rows = remote.query("SELECT id, detected_at, kind, target_id, expected, actual, message, resolved_at, resolved_by FROM drift_log", params![]).await?;
    let mut copied: u64 = 0;
    while let Some(r) = rows.next().await? {
        let id: i64 = r.get(0)?;
        let detected_at: String = r.get(1)?;
        let kind: String = r.get(2)?;
        let target_id: Option<i64> = r.get(3).ok();
        let expected: Option<String> = r.get(4).ok();
        let actual: Option<String> = r.get(5).ok();
        let message: Option<String> = r.get(6).ok();
        let resolved_at: Option<String> = r.get(7).ok();
        let resolved_by: Option<String> = r.get(8).ok();
        let n = local
            .execute(
                "INSERT OR IGNORE INTO drift_log (id, detected_at, kind, target_id, expected, actual, message, resolved_at, resolved_by) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![id, detected_at, kind, target_id, expected, actual, message, resolved_at, resolved_by],
            )
            .await?;
        copied += n;
    }
    Ok(copied)
}

// ---------------------------------------------------------------------------
// Manual sync: push local dirty rows up to the remote.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncSummary {
    pub pushed: u64,
    pub last_synced_at: String,
}

/// Push every `dirty=1` row in `production_event` to the remote, then mark
/// dirty=0 on the local side. INSERT OR IGNORE so re-pushes are safe.
///
/// Returns a summary with the count and the timestamp written to
/// `app_settings.last_synced_at`. On error, dirty=1 stays so the next
/// sync retries naturally.
pub async fn sync_local_to_remote(
    local: &Connection,
    remote: &Connection,
) -> Result<SyncSummary> {
    // Defensive: ensure remote schema is current. Cheap idempotent no-op
    // if it's already at the latest version.
    run_migrations(remote).await?;

    let mut pushed: u64 = 0;
    let mut ids_pushed: Vec<i64> = Vec::new();

    // Fetch dirty rows from local. SELECT into Vec first so we don't hold
    // the rows iterator across remote.execute() awaits.
    let mut rows = local
        .query(
            "SELECT id, recv_date, prod_date, batch,
                    shift_id, grade_id, plant_id, warehouse_id, source_location_id,
                    weight_kg, disposition_kind, partner_equipment_id,
                    flec_count, whse_side, flec_stat, dvo_batch_id,
                    unique_tag, notes, created_at, updated_at
             FROM production_event WHERE dirty = 1
             ORDER BY id",
            params![],
        )
        .await?;
    let mut buffered: Vec<libsql::Value> = Vec::new();
    let mut row_payloads: Vec<Vec<libsql::Value>> = Vec::new();
    while let Some(r) = rows.next().await? {
        let mut payload = Vec::with_capacity(20);
        for i in 0..20 {
            payload.push(r.get_value(i)?);
        }
        let id_val = payload[0].clone();
        if let libsql::Value::Integer(n) = id_val {
            ids_pushed.push(n);
        }
        row_payloads.push(payload);
        buffered.clear(); // unused; just here to silence borrow if we later interleave
    }
    drop(rows);

    for payload in &row_payloads {
        // Remote insert. dirty=0 on the remote (local-side flag has no
        // meaning over there).
        let mut p = payload.clone();
        // Replace position 18 (created_at) and 19 (updated_at) — they're
        // already in the payload at positions 18 and 19, leave them.
        // Append a literal 0 for dirty between updated_at and the rest if
        // we ever change column order. For the current schema, dirty
        // sits at position 18 in the table; we DON'T select it above
        // (we only get the 'truthful' row data, dirty is local-only).
        p.insert(18, libsql::Value::Integer(0)); // dirty for the remote
        let _ = remote
            .execute(
                "INSERT OR IGNORE INTO production_event (
                    id, recv_date, prod_date, batch,
                    shift_id, grade_id, plant_id, warehouse_id, source_location_id,
                    weight_kg, disposition_kind, partner_equipment_id,
                    flec_count, whse_side, flec_stat, dvo_batch_id,
                    unique_tag, notes, dirty, created_at, updated_at
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                p,
            )
            .await?;
        pushed += 1;
    }

    // Mark local rows as no-longer-dirty.
    if !ids_pushed.is_empty() {
        let placeholders = (0..ids_pushed.len()).map(|_| "?").collect::<Vec<_>>().join(",");
        let stmt = format!(
            "UPDATE production_event SET dirty = 0 WHERE id IN ({placeholders})"
        );
        let mut bind: Vec<libsql::Value> = ids_pushed
            .iter()
            .map(|&i| libsql::Value::Integer(i))
            .collect();
        local.execute(&stmt, bind.drain(..).collect::<Vec<_>>()).await?;
    }

    let now = chrono::Utc::now().to_rfc3339();
    upsert_app_setting(local, "last_synced_at", &now).await?;
    Ok(SyncSummary {
        pushed,
        last_synced_at: now,
    })
}

async fn upsert_app_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO app_settings (key, value, updated_at) VALUES (?, ?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, value, chrono::Utc::now().to_rfc3339()],
    )
    .await?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncStatus {
    pub last_synced_at: Option<String>,
    pub pending_count: i64,
}

pub async fn sync_status(local: &Connection) -> Result<SyncStatus> {
    let mut rows = local
        .query(
            "SELECT value FROM app_settings WHERE key = 'last_synced_at'",
            params![],
        )
        .await?;
    let last_synced_at: Option<String> = match rows.next().await? {
        Some(r) => Some(r.get(0)?),
        None => None,
    };
    drop(rows);

    let mut rows = local
        .query(
            "SELECT COUNT(*) FROM production_event WHERE dirty = 1",
            params![],
        )
        .await?;
    let pending_count: i64 = rows
        .next()
        .await?
        .ok_or_else(|| CodoError::internal("pending count returned no rows"))?
        .get(0)?;
    Ok(SyncStatus {
        last_synced_at,
        pending_count,
    })
}
