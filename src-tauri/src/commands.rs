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

/// Open the runtime DB connection (currently direct-remote per the §3
/// deviation; will become a libSQL embedded replica again when Turso Sync
/// stabilizes). Stores the connection in `AppState`.
#[tauri::command]
pub async fn init_local_replica(state: State<'_, AppState>) -> Result<()> {
    // If the connection is already open this session, skip reopening — saves
    // a network roundtrip on every boot resume after the first.
    {
        let conn_guard = state.conn.lock().await;
        if conn_guard.is_some() {
            return Ok(());
        }
    }

    let creds: TursoCreds = credentials::require_via(&state).await?;
    let (database, conn) = db::open_local_replica(&creds).await?;
    // Defensive: ensure migrations are applied against the connection codo
    // will use. Idempotent (run_migrations checks schema_version).
    db::run_migrations(&conn).await?;

    let mut conn_guard = state.conn.lock().await;
    *conn_guard = Some(conn);
    let mut db_guard = state.db.lock().await;
    *db_guard = Some(database);
    Ok(())
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
    let guard = state.conn.lock().await;
    let conn = guard
        .as_ref()
        .ok_or_else(|| CodoError::internal("connection not initialized"))?;

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
    let guard = state.conn.lock().await;
    let conn = guard
        .as_ref()
        .ok_or_else(|| CodoError::internal("connection not initialized"))?;

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
    let guard = state.conn.lock().await;
    let conn = guard
        .as_ref()
        .ok_or_else(|| CodoError::internal("connection not initialized"))?;
    insert_production_event(conn, &input).await
}

/// Pure-of-Tauri version of `create_production_event` — the command above
/// just looks up the connection from `AppState` and forwards. Exposed `pub`
/// so integration tests can drive the full canonicalize → validate →
/// compute_unique_tag → insert pipeline against a tmp libSQL DB.
pub async fn insert_production_event(
    conn: &libsql::Connection,
    input: &CreateProductionEventInput,
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

    // 3. Run the §7.1 row-level validity matrix.
    validate_production_event(EventShape {
        disposition,
        source,
        warehouse,
        plant,
    })?;

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
// list_recent_events
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_recent_events(
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Vec<ProductionEventRow>> {
    let limit = limit.unwrap_or(20).clamp(1, 200);

    let guard = state.conn.lock().await;
    let conn = guard
        .as_ref()
        .ok_or_else(|| CodoError::internal("connection not initialized"))?;

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

struct UniqueTagInput<'a> {
    recv_date: NaiveDate,
    prod_date: Option<NaiveDate>,
    batch: &'a str,
    shift: Option<&'a Shift>,
    grade: Grade,
    plant: Option<&'a Plant>,
    warehouse: Option<&'a Warehouse>,
    whse_side: Option<&'a Side>,
    source: SourceCode,
    disposition: Disposition,
}

fn compute_unique_tag(i: &UniqueTagInput) -> String {
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
