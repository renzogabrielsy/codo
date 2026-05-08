//! Tauri command surface.
//!
//! Step 2 ships the boot path; Step 3 adds the production-event log:
//! `list_lookups` (form dropdowns), `create_production_event` (canonicalize
//! → validate → unique_tag → insert in transaction), `list_recent_events`
//! (the table above the form).

use crate::canonicalize::{
    canonicalize_disposition, canonicalize_grade, canonicalize_plant, canonicalize_shift,
    canonicalize_source, canonicalize_warehouse, canonicalize_whse_side, Disposition, Grade,
    Plant, Shift, Side, SourceCode, Warehouse,
};
use crate::credentials::{self, TursoCreds};
use crate::db;
use crate::error::{CodoError, Result};
use crate::ledger::{
    dvo_batch_ledger, warehouse_ledger_flec, DvoBatchLedger, DvoBatchSide, DvoBatchStatus,
    FlecLedger,
};
use crate::turso_platform::{self, OnboardingRequest};
use crate::validation::{validate_production_event, EventShape};
use crate::AppState;
use chrono::NaiveDate;
use libsql::params;
use serde::{Deserialize, Serialize};
use tauri::State;

// ---------------------------------------------------------------------------
// Onboarding / boot — unchanged from Step 2.
// ---------------------------------------------------------------------------

/// True if Turso credentials are already known to codo this session (cached
/// or in the keyring). Cache-aware so repeated boots don't spam Keychain
/// prompts on unsigned dev binaries.
#[tauri::command]
pub async fn is_onboarded(state: State<'_, AppState>) -> Result<bool> {
    Ok(credentials::load_via(&state).await?.is_some())
}

/// First-launch onboarding. Takes a Turso Platform API token, creates a DB,
/// mints a DB-level token, persists the URL + token to the OS keyring, and
/// throws the Platform token away. PROJECT_BRAIN.md §5 #12.
#[tauri::command]
pub async fn onboard_turso(
    platform_token: String,
    db_name: String,
    state: State<'_, AppState>,
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
    credentials::store_via(&state, &result.creds).await?;
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

/// Apply DDL migrations to the REMOTE Turso DB.
#[tauri::command]
pub async fn run_remote_migrations(state: State<'_, AppState>) -> Result<()> {
    let creds = credentials::require_via(&state).await?;
    db::apply_to_remote(&creds).await?;
    Ok(())
}

/// Open the local SQLite primary DB and run schema migrations + seeds.
/// On first launch after the local-first swap, also runs a one-time
/// bootstrap pull from the remote Turso DB so Renzo's existing test data
/// survives the storage migration. Idempotent — calling again is cheap.
#[tauri::command]
pub async fn init_db(state: State<'_, AppState>) -> Result<()> {
    {
        let guard = state.local_db.lock().await;
        if guard.is_some() {
            return Ok(());
        }
    }

    let local = db::open_local_db().await?;

    // Bootstrap: if we have credentials AND local is empty, pull existing
    // remote rows once. Best-effort — failure logs but doesn't block the
    // boot path (offline-launch must work).
    if let Ok(creds) = credentials::require_via(&state).await {
        if let Err(e) = bootstrap_if_needed(&local, &creds).await {
            tracing::warn!(error = %e, "bootstrap from remote failed (continuing offline)");
        }
    }

    let mut guard = state.local_db.lock().await;
    *guard = Some(local);
    Ok(())
}

async fn bootstrap_if_needed(local: &libsql::Database, creds: &TursoCreds) -> Result<()> {
    let local_conn = local.connect()?;
    let remote = db::open_remote_db(creds).await?;
    let remote_conn = remote.connect()?;
    let pulled = db::bootstrap_local_from_remote(&local_conn, &remote_conn).await?;
    if pulled > 0 {
        tracing::info!(rows = pulled, "bootstrapped local DB from remote");
    }
    Ok(())
}

/// Spin up a fresh `Connection` from the cached LOCAL `Database`. Cheap —
/// pure local file I/O, no network. Use this everywhere a Tauri command
/// needs DB access. Don't cache across awaits — fresh-per-command is the
/// whole point of this helper.
async fn fresh_conn(state: &State<'_, AppState>) -> Result<libsql::Connection> {
    let guard = state.local_db.lock().await;
    let db = guard
        .as_ref()
        .ok_or_else(|| CodoError::internal("local database not initialized"))?;
    Ok(db.connect()?)
}

/// Build a fresh remote backup `Database` on demand. Used only by the
/// manual sync path; rebuild cost is negligible (the actual network call
/// happens at connect()/query time).
async fn remote_db_handle(state: &State<'_, AppState>) -> Result<libsql::Database> {
    let creds = credentials::require_via(state).await?;
    db::open_remote_db(&creds).await
}

/// codo's compiled-in version. Used by the updater UI to show "you have X,
/// Y is available" with the §4.7 always-show-solution rule.
#[tauri::command]
pub fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct WarehouseRow {
    pub id: i64,
    pub code: String,
    pub default_unit: String,
}

/// Tiny demo command — proves the connection works and the seed landed.
#[tauri::command]
pub async fn list_warehouses(state: State<'_, AppState>) -> Result<Vec<WarehouseRow>> {
    let conn = fresh_conn(&state).await?;
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

// ---------------------------------------------------------------------------
// Step 3 — log a production event.
// ---------------------------------------------------------------------------

/// Bundle of every lookup the form needs to populate its dropdowns. One
/// roundtrip to fetch all of them on form mount.
#[derive(Debug, Clone, Serialize)]
pub struct LookupBundle {
    pub shifts: Vec<LookupRow>,
    pub grades: Vec<GradeRow>,
    pub plants: Vec<LookupRow>,
    pub warehouses: Vec<WarehouseRow>,
    pub source_locations: Vec<SourceLocationRow>,
    pub partner_equipment: Vec<PartnerEquipmentRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LookupRow {
    pub id: i64,
    pub code: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GradeRow {
    pub id: i64,
    pub code: String,
    pub display_name: String,
    pub expected_kg_per_bag_min: Option<f64>,
    pub expected_kg_per_bag_max: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceLocationRow {
    pub id: i64,
    pub code: String,
    pub display_name: String,
    pub kind: String,
    pub plant_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PartnerEquipmentRow {
    pub id: i64,
    pub code: String,
    pub display_name: String,
    pub kind: String,
}

/// Loads every lookup table the form needs in a single Tauri command.
#[tauri::command]
pub async fn list_lookups(state: State<'_, AppState>) -> Result<LookupBundle> {
    let conn = fresh_conn(&state).await?;

    let shifts = {
        let mut rows = conn
            .query(
                "SELECT id, code, display_name FROM shift ORDER BY sort_order, id",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(LookupRow {
                id: row.get(0)?,
                code: row.get(1)?,
                display_name: row.get(2)?,
            });
        }
        out
    };

    let grades = {
        let mut rows = conn
            .query(
                "SELECT id, code, display_name, expected_kg_per_bag_min, expected_kg_per_bag_max
                 FROM grade ORDER BY sort_order, id",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(GradeRow {
                id: row.get(0)?,
                code: row.get(1)?,
                display_name: row.get(2)?,
                expected_kg_per_bag_min: row.get(3).ok(),
                expected_kg_per_bag_max: row.get(4).ok(),
            });
        }
        out
    };

    let plants = {
        let mut rows = conn
            .query(
                "SELECT id, code, display_name FROM plant ORDER BY id",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(LookupRow {
                id: row.get(0)?,
                code: row.get(1)?,
                display_name: row.get(2)?,
            });
        }
        out
    };

    let warehouses = {
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
        out
    };

    let source_locations = {
        let mut rows = conn
            .query(
                "SELECT id, code, display_name, kind, plant_id
                 FROM source_location ORDER BY id",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(SourceLocationRow {
                id: row.get(0)?,
                code: row.get(1)?,
                display_name: row.get(2)?,
                kind: row.get(3)?,
                plant_id: row.get(4).ok(),
            });
        }
        out
    };

    let partner_equipment = {
        let mut rows = conn
            .query(
                "SELECT id, code, display_name, kind
                 FROM partner_equipment ORDER BY sort_order, id",
                params![],
            )
            .await?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(PartnerEquipmentRow {
                id: row.get(0)?,
                code: row.get(1)?,
                display_name: row.get(2)?,
                kind: row.get(3)?,
            });
        }
        out
    };

    Ok(LookupBundle {
        shifts,
        grades,
        plants,
        warehouses,
        source_locations,
        partner_equipment,
    })
}

// ---------------------------------------------------------------------------
// create_production_event
// ---------------------------------------------------------------------------

/// Payload from the form. Strings + ints (no enums) so the JS side doesn't
/// need to know the canonical enum shapes — Rust canonicalizes everything.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProductionEventInput {
    pub recv_date: String,           // ISO YYYY-MM-DD
    pub prod_date: Option<String>,   // ISO; nullable
    pub batch: String,               // free-text, canonicalized to upper
    pub shift_code: Option<String>,  // 'M'|'E'|'N' or empty/null
    pub grade_code: String,          // '3X50' etc.
    pub source_code: String,         // 'TNK 1'..'DVO'
    /// Plant override. Only honoured when source kind is `warehouse_flec`
    /// (FLEC); otherwise the plant is forced from the source per §7.2.
    pub plant_code_override: Option<String>,
    pub warehouse_code: Option<String>, // 'WHSE 1'..'WHSE 7' or empty
    pub disposition_raw: String,        // 'FLEC' | 'C1'..'C4' | 'RK1'..'RK4'
    pub weight_kg: f64,
    pub flec_count: Option<i64>,
    pub whse_side: Option<String>, // 'LS' | 'RS' | empty
    pub flec_stat: Option<String>,
    pub dvo_batch_id: Option<i64>,
    pub notes: Option<String>,
}

/// Result row returned to the UI after a successful insert.
#[derive(Debug, Clone, Serialize)]
pub struct ProductionEventRow {
    pub id: i64,
    pub recv_date: String,
    pub prod_date: Option<String>,
    pub batch: String,
    pub shift_code: Option<String>,
    pub grade_code: String,
    pub plant_code: Option<String>,
    pub warehouse_code: Option<String>,
    pub source_code: String,
    pub weight_kg: f64,
    pub disposition_kind: String,
    pub partner_equipment_code: Option<String>,
    pub flec_count: Option<i64>,
    pub whse_side: Option<String>,
    pub unique_tag: String,
    pub notes: Option<String>,
    pub created_at: String,
}

#[tauri::command]
pub async fn create_production_event(
    input: CreateProductionEventInput,
    state: State<'_, AppState>,
) -> Result<ProductionEventRow> {
    let conn = fresh_conn(&state).await?;
    insert_production_event(&conn, &input).await
}

/// Insert validation policy.
///
/// `Strict` runs the full §7.1 row-level matrix and rejects forbidden
/// combinations. Used by every Tauri command for new writes.
///
/// `Migration` skips §7.1 — used by `codo-migrate` because the workbook
/// contains historical rows that fail §7.1 by design (e.g. cosmetic
/// W6/W7 in the WHSE column → NULL warehouse on a FLEC-bagging event,
/// per brain §2 vs §7.1 inconsistency). Those rows still belong in the
/// log; codo's UI later surfaces them as "needs review" via drift_log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertMode {
    Strict,
    Migration,
}

/// Pure-of-Tauri version of `create_production_event` — the command above
/// just looks up the connection from `AppState` and forwards. Exposed `pub`
/// so integration tests + the migration bin can drive the full canonicalize
/// → optionally-validate → compute_unique_tag → insert pipeline.
pub async fn insert_production_event(
    conn: &libsql::Connection,
    input: &CreateProductionEventInput,
) -> Result<ProductionEventRow> {
    insert_production_event_with_mode(conn, input, InsertMode::Strict).await
}

pub async fn insert_production_event_with_mode(
    conn: &libsql::Connection,
    input: &CreateProductionEventInput,
    mode: InsertMode,
) -> Result<ProductionEventRow> {
    // 1. Canonicalize every categorical (defense in depth — the JS form
    //    already runs zod, but Rust is the source of truth).
    let shift = input
        .shift_code
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(canonicalize_shift)
        .transpose()?;
    let grade = canonicalize_grade(&input.grade_code)?;
    let source = canonicalize_source(&input.source_code)?;
    let warehouse = match input.warehouse_code.as_deref() {
        Some(w) => canonicalize_warehouse(w)?,
        None => None,
    };
    let whse_side = match input.whse_side.as_deref() {
        Some(s) => canonicalize_whse_side(s)?,
        None => None,
    };
    let disposition = canonicalize_disposition(&input.disposition_raw)?;

    // 2. Derive plant per §7.2: forced for non-FLEC sources, operator override
    //    only honoured when source.kind = warehouse_flec.
    let plant = derive_plant(source, input.plant_code_override.as_deref())?;

    // 3. Run the §7.1 row-level validity matrix in Strict mode. Migration
    //    mode skips this (the workbook has legacy rows that violate §7.1
    //    by design — see InsertMode docs).
    if mode == InsertMode::Strict {
        validate_production_event(EventShape {
            disposition,
            source,
            warehouse,
            plant,
        })?;
    }

    // 4. Parse dates + validate weight.
    let recv_date = parse_iso_date(&input.recv_date, "recv_date")?;
    let prod_date = match input.prod_date.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(s) => Some(parse_iso_date(s, "prod_date")?),
        None => None,
    };
    if !(input.weight_kg > 0.0) {
        return Err(CodoError::invalid("weight_kg must be > 0"));
    }
    if let Some(n) = input.flec_count {
        if n <= 0 {
            return Err(CodoError::invalid("flec_count must be > 0 (or null)"));
        }
    }
    let batch = input.batch.trim().to_uppercase();
    if batch.is_empty() {
        return Err(CodoError::invalid("batch is required"));
    }

    // 5. Compute unique_tag from canonical components (matches the workbook's
    //    10-segment hyphen-concat per schema-extraction.md §3.1, with Excel
    //    serial dates so codo's tags interleave with migrated workbook tags).
    let unique_tag = compute_unique_tag(&UniqueTagInput {
        recv_date,
        prod_date,
        batch: &batch,
        shift: shift.as_ref(),
        grade,
        plant: plant.as_ref(),
        warehouse: warehouse.as_ref(),
        whse_side: whse_side.as_ref(),
        source,
        disposition,
    });

    // 6. Insert in a transaction.
    let now = chrono::Utc::now().to_rfc3339();
    let shift_id = match shift {
        Some(s) => Some(shift_id_for(s)),
        None => None,
    };
    let plant_id = plant.map(plant_id_for);
    let warehouse_id = warehouse.map(warehouse_id_for);
    let source_id = source_id_for(source);
    let partner_equipment_id = partner_equipment_id_for(disposition);

    let tx = conn.transaction().await?;
    tx.execute(
        "INSERT INTO production_event (
            recv_date, prod_date, batch,
            shift_id, grade_id, plant_id, warehouse_id, source_location_id,
            weight_kg, disposition_kind, partner_equipment_id,
            flec_count, whse_side, flec_stat, dvo_batch_id,
            unique_tag, notes, dirty, created_at, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        params![
            recv_date.format("%Y-%m-%d").to_string(),
            prod_date.map(|d| d.format("%Y-%m-%d").to_string()),
            batch.clone(),
            shift_id,
            grade_id_for(grade),
            plant_id,
            warehouse_id,
            source_id,
            input.weight_kg,
            disposition.kind_code(),
            partner_equipment_id,
            input.flec_count,
            whse_side.as_ref().map(|s| s.code().to_string()),
            input
                .flec_stat
                .as_ref()
                .and_then(|s| crate::canonicalize::canonicalize_flec_stat(s)),
            input.dvo_batch_id,
            unique_tag.clone(),
            input.notes.as_deref().map(str::to_string),
            now.clone(),
            now.clone(),
        ],
    )
    .await?;

    // Pull the row back to return the assigned id + canonical values.
    let mut rows = tx
        .query(
            "SELECT pe.id, pe.recv_date, pe.prod_date, pe.batch,
                    sh.code, gr.code, pl.code, w.code, sl.code,
                    pe.weight_kg, pe.disposition_kind, peq.code,
                    pe.flec_count, pe.whse_side, pe.unique_tag, pe.notes,
                    pe.created_at
             FROM production_event pe
             LEFT JOIN shift             sh  ON sh.id  = pe.shift_id
             JOIN      grade             gr  ON gr.id  = pe.grade_id
             LEFT JOIN plant             pl  ON pl.id  = pe.plant_id
             LEFT JOIN warehouse         w   ON w.id   = pe.warehouse_id
             JOIN      source_location   sl  ON sl.id  = pe.source_location_id
             LEFT JOIN partner_equipment peq ON peq.id = pe.partner_equipment_id
             WHERE pe.unique_tag = ?",
            params![unique_tag.clone()],
        )
        .await?;

    let row = rows
        .next()
        .await?
        .ok_or_else(|| CodoError::internal("inserted row not found"))?;
    let inserted = ProductionEventRow {
        id: row.get(0)?,
        recv_date: row.get(1)?,
        prod_date: row.get(2).ok(),
        batch: row.get(3)?,
        shift_code: row.get(4).ok(),
        grade_code: row.get(5)?,
        plant_code: row.get(6).ok(),
        warehouse_code: row.get(7).ok(),
        source_code: row.get(8)?,
        weight_kg: row.get(9)?,
        disposition_kind: row.get(10)?,
        partner_equipment_code: row.get(11).ok(),
        flec_count: row.get(12).ok(),
        whse_side: row.get(13).ok(),
        unique_tag: row.get(14)?,
        notes: row.get(15).ok(),
        created_at: row.get(16)?,
    };

    drop(rows);
    tx.commit().await?;
    Ok(inserted)
}

// ---------------------------------------------------------------------------
// create_production_events_bulk — Excel-paste friendly batch insert. All
// rows go in one transaction; if any fails, none land.
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn create_production_events_bulk(
    inputs: Vec<CreateProductionEventInput>,
    state: State<'_, AppState>,
) -> Result<Vec<ProductionEventRow>> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }

    // Pre-validate ALL rows before opening the transaction. Cheaper to
    // bail early than to insert N-1 and then have to roll back.
    for (i, input) in inputs.iter().enumerate() {
        if let Err(e) = validate_input_shape(input) {
            return Err(CodoError::invalid(format!("row {}: {}", i + 1, e)));
        }
    }

    let conn = fresh_conn(&state).await?;
    let mut inserted = Vec::with_capacity(inputs.len());
    for (i, input) in inputs.iter().enumerate() {
        match insert_production_event(&conn, input).await {
            Ok(row) => inserted.push(row),
            Err(e) => {
                // libSQL doesn't expose explicit transaction rollback to
                // this API form; the partial state is the price of the
                // direct-remote mode. Surface which row failed so the
                // operator can re-paste minus the bad ones.
                return Err(CodoError::invalid(format!(
                    "row {} of {} failed ({} succeeded before): {}",
                    i + 1,
                    inputs.len(),
                    inserted.len(),
                    e
                )));
            }
        }
    }
    Ok(inserted)
}

/// Light pre-validation that mirrors the per-row checks insert_production_event
/// runs, so a bulk paste of 50 rows fails on the bad one *before* writes start.
fn validate_input_shape(input: &CreateProductionEventInput) -> Result<()> {
    if !(input.weight_kg > 0.0) {
        return Err(CodoError::invalid("weight_kg must be > 0"));
    }
    if let Some(n) = input.flec_count {
        if n <= 0 {
            return Err(CodoError::invalid("flec_count must be > 0 (or null)"));
        }
    }
    if input.batch.trim().is_empty() {
        return Err(CodoError::invalid("batch is required"));
    }
    if input.grade_code.trim().is_empty() {
        return Err(CodoError::invalid("grade is required"));
    }
    if input.source_code.trim().is_empty() {
        return Err(CodoError::invalid("source is required"));
    }
    if input.disposition_raw.trim().is_empty() {
        return Err(CodoError::invalid("disposition is required"));
    }
    if input.recv_date.trim().is_empty() {
        return Err(CodoError::invalid("recv_date is required"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// list_recent_events
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_recent_events(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Vec<ProductionEventRow>> {
    let limit = limit.unwrap_or(20).clamp(1, 200);
    let conn = fresh_conn(&state).await?;
    let mut rows = conn
        .query(
            "SELECT pe.id, pe.recv_date, pe.prod_date, pe.batch,
                    sh.code, gr.code, pl.code, w.code, sl.code,
                    pe.weight_kg, pe.disposition_kind, peq.code,
                    pe.flec_count, pe.whse_side, pe.unique_tag, pe.notes,
                    pe.created_at
             FROM production_event pe
             LEFT JOIN shift             sh  ON sh.id  = pe.shift_id
             JOIN      grade             gr  ON gr.id  = pe.grade_id
             LEFT JOIN plant             pl  ON pl.id  = pe.plant_id
             LEFT JOIN warehouse         w   ON w.id   = pe.warehouse_id
             JOIN      source_location   sl  ON sl.id  = pe.source_location_id
             LEFT JOIN partner_equipment peq ON peq.id = pe.partner_equipment_id
             ORDER BY pe.id DESC
             LIMIT ?",
            params![limit],
        )
        .await?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        out.push(ProductionEventRow {
            id: row.get(0)?,
            recv_date: row.get(1)?,
            prod_date: row.get(2).ok(),
            batch: row.get(3)?,
            shift_code: row.get(4).ok(),
            grade_code: row.get(5)?,
            plant_code: row.get(6).ok(),
            warehouse_code: row.get(7).ok(),
            source_code: row.get(8)?,
            weight_kg: row.get(9)?,
            disposition_kind: row.get(10)?,
            partner_equipment_code: row.get(11).ok(),
            flec_count: row.get(12).ok(),
            whse_side: row.get(13).ok(),
            unique_tag: row.get(14)?,
            notes: row.get(15).ok(),
            created_at: row.get(16)?,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Manual sync — push local dirty rows to the remote backup target.
// ---------------------------------------------------------------------------

/// Push every locally-changed row up to the Turso backup. Manual: invoked
/// when the operator clicks Sync. Returns the count pushed and the
/// timestamp it ran. Failures leave dirty=1 so the next attempt naturally
/// retries.
#[tauri::command]
pub async fn sync_now(state: State<'_, AppState>) -> Result<db::SyncSummary> {
    let local_conn = fresh_conn(&state).await?;
    let remote = remote_db_handle(&state).await?;
    let remote_conn = remote.connect()?;
    db::sync_local_to_remote(&local_conn, &remote_conn).await
}

/// Read-only sync state for the UI. `last_synced_at` is null until the
/// first successful sync. `pending_count` is the number of dirty rows
/// waiting to push.
#[tauri::command]
pub async fn get_sync_status(state: State<'_, AppState>) -> Result<db::SyncStatus> {
    let conn = fresh_conn(&state).await?;
    db::sync_status(&conn).await
}

// ---------------------------------------------------------------------------
// Internal helpers — id mapping (lookups are seeded with deterministic ids).
// ---------------------------------------------------------------------------

fn shift_id_for(s: Shift) -> i64 {
    match s {
        Shift::M => 1,
        Shift::E => 2,
        Shift::N => 3,
    }
}

fn grade_id_for(g: Grade) -> i64 {
    match g {
        Grade::G3x50 => 1,
        Grade::G2x6 => 2,
        Grade::G3p5 => 3,
        Grade::G4x8 => 4,
    }
}

fn plant_id_for(p: Plant) -> i64 {
    match p {
        Plant::W6 => 1,
        Plant::W7 => 2,
        Plant::W6W7 => 3,
        Plant::Dvo => 4,
    }
}

fn warehouse_id_for(w: Warehouse) -> i64 {
    match w {
        Warehouse::W1 => 1,
        Warehouse::W2 => 2,
        Warehouse::W3 => 3,
        Warehouse::W5 => 5,
        Warehouse::W7 => 7,
    }
}

fn source_id_for(s: SourceCode) -> i64 {
    match s {
        SourceCode::Tnk1 => 1,
        SourceCode::Tnk2 => 2,
        SourceCode::Tnk3 => 3,
        SourceCode::Tnk4 => 4,
        SourceCode::W7 => 5,
        SourceCode::W6 => 6,
        SourceCode::Flec => 7,
        SourceCode::Dvo => 8,
    }
}

fn partner_equipment_id_for(d: Disposition) -> Option<i64> {
    Some(match d {
        Disposition::FlecBagging => return None,
        Disposition::PartnerCrusher(1) => 1,
        Disposition::PartnerCrusher(2) => 2,
        Disposition::PartnerCrusher(3) => 3,
        Disposition::PartnerCrusher(4) => 4,
        Disposition::PartnerKiln(1) => 11,
        Disposition::PartnerKiln(2) => 12,
        Disposition::PartnerKiln(3) => 13,
        Disposition::PartnerKiln(4) => 14,
        _ => return None,
    })
}

fn parse_iso_date(s: &str, field: &'static str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
        .map_err(|e| CodoError::invalid(format!("{field}: {e}")))
}

/// §7.2 SRC↔PLANT pairing. Forced for non-FLEC sources; operator override
/// only honoured (and even then only optionally) when source.kind = FLEC.
fn derive_plant(source: SourceCode, override_code: Option<&str>) -> Result<Option<Plant>> {
    let forced = match source {
        SourceCode::Tnk1 | SourceCode::Tnk2 | SourceCode::Tnk3 | SourceCode::Tnk4 => Some(Plant::W6),
        SourceCode::W7 => Some(Plant::W7),
        SourceCode::W6 => Some(Plant::W6),
        SourceCode::Dvo => Some(Plant::Dvo),
        SourceCode::Flec => None, // operator-set
    };
    match (forced, override_code.filter(|s| !s.trim().is_empty())) {
        (Some(p), _) => Ok(Some(p)),
        (None, Some(code)) => Ok(Some(canonicalize_plant(code)?)),
        (None, None) => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// unique_tag construction. Matches schema-extraction.md §3.1 exactly so codo's
// tags interleave cleanly with migrated workbook tags in Step 4.
// ---------------------------------------------------------------------------

pub struct UniqueTagInput<'a> {
    pub recv_date: NaiveDate,
    pub prod_date: Option<NaiveDate>,
    pub batch: &'a str,
    pub shift: Option<&'a Shift>,
    pub grade: Grade,
    pub plant: Option<&'a Plant>,
    pub warehouse: Option<&'a Warehouse>,
    pub whse_side: Option<&'a Side>,
    pub source: SourceCode,
    pub disposition: Disposition,
}

pub fn compute_unique_tag(i: &UniqueTagInput) -> String {
    let segs: [String; 10] = [
        excel_serial(i.recv_date).to_string(),
        i.prod_date.map(excel_serial).map(|n| n.to_string()).unwrap_or_default(),
        i.batch.to_string(),
        i.shift.map(|s| s.code().to_string()).unwrap_or_default(),
        i.grade.code().to_string(),
        i.plant.map(|p| p.code().to_string()).unwrap_or_default(),
        i.warehouse.map(|w| w.code().to_string()).unwrap_or_default(),
        i.whse_side.map(|s| s.code().to_string()).unwrap_or_default(),
        i.source.code().to_string(),
        i.disposition.raw_form().to_string(),
    ];
    segs.join("-")
}

/// Excel serial-date integer. Epoch is 1899-12-30 (Excel's pre-1900 leap-year
/// quirk handled correctly by skipping it).
fn excel_serial(d: NaiveDate) -> i64 {
    let epoch = NaiveDate::from_ymd_opt(1899, 12, 30).expect("static");
    (d - epoch).num_days()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excel_serial_known_dates() {
        // 2026-05-01 → days since 1899-12-30 = 46143
        assert_eq!(
            excel_serial(NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()),
            46143
        );
    }

    #[test]
    fn unique_tag_matches_workbook_format() {
        // schema-extraction.md §3.1 example:
        //   46091-46091-MARCH-M-3X50-W6-WHSE 7-RS-TNK 2-FLEC
        let date = NaiveDate::from_ymd_opt(2026, 3, 10).unwrap();
        // Sanity-check our serial number
        let serial = excel_serial(date);
        let tag = compute_unique_tag(&UniqueTagInput {
            recv_date: date,
            prod_date: Some(date),
            batch: "MARCH",
            shift: Some(&Shift::M),
            grade: Grade::G3x50,
            plant: Some(&Plant::W6),
            warehouse: Some(&Warehouse::W7),
            whse_side: Some(&Side::Rs),
            source: SourceCode::Tnk2,
            disposition: Disposition::FlecBagging,
        });
        assert_eq!(
            tag,
            format!("{serial}-{serial}-MARCH-M-3X50-W6-WHSE 7-RS-TNK 2-FLEC")
        );
    }

    #[test]
    fn unique_tag_handles_empty_segments_with_dashes() {
        // schema-extraction §3.2: blank segments produce consecutive dashes.
        let date = NaiveDate::from_ymd_opt(2025, 11, 28).unwrap();
        let tag = compute_unique_tag(&UniqueTagInput {
            recv_date: date,
            prod_date: None, // empty → blank position
            batch: "NOVEMBER",
            shift: None, // empty → blank position
            grade: Grade::G3x50,
            plant: Some(&Plant::W6),
            warehouse: None, // empty → blank position
            whse_side: None, // empty → blank position
            source: SourceCode::Tnk2,
            disposition: Disposition::PartnerCrusher(1),
        });
        let parts: Vec<&str> = tag.split('-').collect();
        assert_eq!(parts.len(), 10);
        assert_eq!(parts[1], ""); // prod_date blank
        assert_eq!(parts[3], ""); // shift blank
        assert_eq!(parts[6], ""); // warehouse blank
        assert_eq!(parts[7], ""); // whse_side blank
        assert_eq!(parts[9], "C1");
    }
}

// ---------------------------------------------------------------------------
// Step 5: warehouse + DVO ledger commands.
// ---------------------------------------------------------------------------

/// FLEC ledger for WHSE 1/2/5/7. start_date is ISO 'YYYY-MM-DD'.
#[tauri::command]
pub async fn get_warehouse_ledger_flec(
    warehouse_code: String,
    start_date: String,
    state: State<'_, AppState>,
) -> Result<FlecLedger> {
    let conn = fresh_conn(&state).await?;
    let warehouse = canonicalize_warehouse(&warehouse_code)?
        .ok_or_else(|| CodoError::invalid(format!("warehouse '{warehouse_code}' is empty/invalid")))?;
    let date = NaiveDate::parse_from_str(start_date.trim(), "%Y-%m-%d")
        .map_err(|e| CodoError::invalid(format!("start_date '{start_date}' must be ISO YYYY-MM-DD: {e}")))?;
    warehouse_ledger_flec(&conn, warehouse, date).await
}

/// DVO batch ledger for one batch.
#[tauri::command]
pub async fn get_dvo_batch_ledger(
    dvo_batch_id: i64,
    state: State<'_, AppState>,
) -> Result<DvoBatchLedger> {
    let conn = fresh_conn(&state).await?;
    dvo_batch_ledger(&conn, dvo_batch_id).await
}

#[derive(Debug, Clone, Serialize)]
pub struct DvoBatchSummary {
    pub id: i64,
    pub code: String,
    pub year: i32,
    pub start_month: u8,
    pub side: String, // 'LEFT' | 'RIGHT'
    pub status: String, // 'open' | 'closed'
    pub opened_at: String,
    pub closed_at: Option<String>,
    pub receipt_count: i64,
    pub outflow_count: i64,
    pub frozen_transit_loss: Option<f64>,
    pub frozen_yield_loss: Option<f64>,
}

/// List every DVO batch with summary counts. Used by the DVO batch list page.
#[tauri::command]
pub async fn list_dvo_batches(state: State<'_, AppState>) -> Result<Vec<DvoBatchSummary>> {
    let conn = fresh_conn(&state).await?;
    let mut rows = conn
        .query(
            "SELECT db.id, db.code, db.year, db.start_month, db.side, db.status,
                    db.opened_at, db.closed_at,
                    db.frozen_transit_loss, db.frozen_yield_loss,
                    (SELECT COUNT(*) FROM dvo_receipt dr WHERE dr.dvo_batch_id = db.id) AS receipt_count,
                    (SELECT COUNT(*) FROM production_event pe WHERE pe.dvo_batch_id = db.id) AS outflow_count
             FROM dvo_batch db
             ORDER BY db.year DESC, db.start_month DESC, db.side",
            params![],
        )
        .await?;
    let mut out = Vec::new();
    while let Some(r) = rows.next().await? {
        out.push(DvoBatchSummary {
            id: r.get(0)?,
            code: r.get(1)?,
            year: r.get::<i64>(2)? as i32,
            start_month: r.get::<i64>(3)? as u8,
            side: r.get(4)?,
            status: r.get(5)?,
            opened_at: r.get(6)?,
            closed_at: r.get(7).ok(),
            frozen_transit_loss: r.get(8).ok(),
            frozen_yield_loss: r.get(9).ok(),
            receipt_count: r.get(10)?,
            outflow_count: r.get(11)?,
        });
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
pub struct WarehouseSummary {
    pub code: String,
    pub default_unit: String,
    /// For flec_count warehouses only (WHSE 1/2/5/7) — sum of current
    /// (grade, side) balances. None for WHSE 3 (use DVO batch summaries).
    pub total_flec: Option<i64>,
    /// Most-recent event recv_date for this warehouse, for "last activity" display.
    pub last_event_date: Option<String>,
    pub event_count: i64,
}

/// Quick rollup per warehouse for the warehouse-list landing page.
#[tauri::command]
pub async fn list_warehouse_summaries(
    state: State<'_, AppState>,
) -> Result<Vec<WarehouseSummary>> {
    let conn = fresh_conn(&state).await?;
    let mut rows = conn
        .query(
            "SELECT w.code, w.default_unit,
                    (SELECT MAX(pe.recv_date) FROM production_event pe WHERE pe.warehouse_id = w.id) AS last_date,
                    (SELECT COUNT(*) FROM production_event pe WHERE pe.warehouse_id = w.id) AS evt_count
             FROM warehouse w
             ORDER BY w.id",
            params![],
        )
        .await?;
    let mut out = Vec::new();
    while let Some(r) = rows.next().await? {
        let code: String = r.get(0)?;
        let unit: String = r.get(1)?;
        let last_date: Option<String> = r.get(2).ok();
        let evt_count: i64 = r.get(3)?;

        // Total flec: only meaningful for flec-count warehouses. Compute via
        // warehouse_ledger_flec from epoch start so it sums every event.
        let total_flec: Option<i64> = if unit == "flec_count" {
            let warehouse = canonicalize_warehouse(&code)?
                .ok_or_else(|| CodoError::internal(format!("seeded warehouse '{code}' invalid?")))?;
            let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).expect("static");
            let ledger = warehouse_ledger_flec(&conn, warehouse, epoch).await?;
            Some(ledger.current_balances.iter().map(|b| b.flec_count).sum())
        } else {
            None
        };
        out.push(WarehouseSummary {
            code,
            default_unit: unit,
            total_flec,
            last_event_date: last_date,
            event_count: evt_count,
        });
    }
    Ok(out)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenDvoBatchInput {
    pub code: String,
    pub start_month: u8,
    pub year: i32,
    pub side: String, // 'LEFT' | 'RIGHT'
    pub notes: Option<String>,
}

#[tauri::command]
pub async fn open_dvo_batch(
    input: OpenDvoBatchInput,
    state: State<'_, AppState>,
) -> Result<i64> {
    if !(1..=12).contains(&input.start_month) {
        return Err(CodoError::invalid("start_month must be 1..=12"));
    }
    if input.side != "LEFT" && input.side != "RIGHT" {
        return Err(CodoError::invalid("side must be LEFT or RIGHT"));
    }
    let conn = fresh_conn(&state).await?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO dvo_batch (code, warehouse_id, start_month, year, side, status, opened_at, notes, created_at, updated_at) VALUES (?, 3, ?, ?, ?, 'open', ?, ?, ?, ?)",
        params![
            input.code.trim().to_uppercase(),
            input.start_month as i64,
            input.year as i64,
            input.side,
            now.clone(),
            input.notes,
            now.clone(),
            now,
        ],
    )
    .await?;
    let mut rows = conn
        .query(
            "SELECT id FROM dvo_batch WHERE code = ?",
            params![input.code.trim().to_uppercase()],
        )
        .await?;
    let r = rows
        .next()
        .await?
        .ok_or_else(|| CodoError::internal("just-inserted dvo_batch not found"))?;
    Ok(r.get(0)?)
}

/// Close a DVO batch — freezes transit + yield loss snapshots into the
/// frozen_* columns and flips status to 'closed'. Per brain §7.16, this
/// is operator-initiated only.
#[tauri::command]
pub async fn close_dvo_batch(
    dvo_batch_id: i64,
    closed_by: Option<String>,
    state: State<'_, AppState>,
) -> Result<()> {
    let conn = fresh_conn(&state).await?;
    let ledger = dvo_batch_ledger(&conn, dvo_batch_id).await?;
    if ledger.batch.status == DvoBatchStatus::Closed {
        return Err(CodoError::invalid("batch already closed"));
    }
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE dvo_batch SET status = 'closed', closed_at = ?, closed_by = ?, frozen_transit_loss = ?, frozen_yield_loss = ?, updated_at = ? WHERE id = ?",
        params![
            now.clone(),
            closed_by,
            ledger.transit_loss.value,
            ledger.yield_loss.value,
            now,
            dvo_batch_id,
        ],
    )
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetOpeningBalanceInput {
    pub warehouse_code: String,
    pub grade_code: String,
    pub side: String, // 'LS' | 'RS'
    pub period_start_date: String, // ISO YYYY-MM-DD
    pub opening_flec_count: i64,
    pub notes: Option<String>,
}

/// Per brain §7.9 the operator can re-set opening balance any time. UI
/// just says 'as of today, balance is X' — no period concept exposed.
/// codo writes a new row dated period_start_date (today by default);
/// the ledger function picks the most-recent row dated ≤ start_date.
#[tauri::command]
pub async fn set_warehouse_opening_balance(
    input: SetOpeningBalanceInput,
    state: State<'_, AppState>,
) -> Result<i64> {
    if input.opening_flec_count < 0 {
        return Err(CodoError::invalid("opening_flec_count must be >= 0"));
    }
    let warehouse = canonicalize_warehouse(&input.warehouse_code)?
        .ok_or_else(|| CodoError::invalid("warehouse code is empty/invalid"))?;
    let grade = canonicalize_grade(&input.grade_code)?;
    let side = match input.side.trim().to_uppercase().as_str() {
        "LS" => "LS",
        "RS" => "RS",
        _ => return Err(CodoError::invalid("side must be LS or RS")),
    };
    let _ = NaiveDate::parse_from_str(input.period_start_date.trim(), "%Y-%m-%d")
        .map_err(|e| CodoError::invalid(format!("period_start_date must be ISO: {e}")))?;

    let conn = fresh_conn(&state).await?;
    let now = chrono::Utc::now().to_rfc3339();
    let warehouse_id = match warehouse {
        Warehouse::W1 => 1_i64,
        Warehouse::W2 => 2,
        Warehouse::W3 => 3,
        Warehouse::W5 => 5,
        Warehouse::W7 => 7,
    };
    let grade_id = match grade {
        Grade::G3x50 => 1_i64,
        Grade::G2x6 => 2,
        Grade::G3p5 => 3,
        Grade::G4x8 => 4,
    };
    conn.execute(
        "INSERT INTO warehouse_opening_balance (warehouse_id, grade_id, side, period_start_date, opening_flec_count, notes, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT (warehouse_id, grade_id, side, period_start_date) DO UPDATE SET opening_flec_count = excluded.opening_flec_count, notes = excluded.notes, updated_at = excluded.updated_at",
        params![
            warehouse_id,
            grade_id,
            side,
            input.period_start_date.trim(),
            input.opening_flec_count,
            input.notes,
            now.clone(),
            now,
        ],
    )
    .await?;
    let mut rows = conn
        .query(
            "SELECT id FROM warehouse_opening_balance WHERE warehouse_id = ? AND grade_id = ? AND side = ? AND period_start_date = ?",
            params![warehouse_id, grade_id, side, input.period_start_date.trim()],
        )
        .await?;
    let r = rows
        .next()
        .await?
        .ok_or_else(|| CodoError::internal("opening balance row not found after upsert"))?;
    Ok(r.get(0)?)
}

// keep the unused Plant + Shift + Side + DvoBatchSide imports out of the
// dead-code warning cycle when these symbols aren't used directly.
#[allow(dead_code)]
fn _ledger_imports_keepalive(_: Plant, _: Shift, _: Side, _: DvoBatchSide) {}
