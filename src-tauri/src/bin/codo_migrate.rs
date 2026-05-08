//! codo-migrate — one-shot Excel-to-codo migration CLI.
//!
//! Step 4 per PROJECT_BRAIN.md §9. Reads `2025 CI PRODUCTION V2.xlsb` and
//! pipes every row through codo's existing canonicalize → §7.2 plant
//! derivation → §7.1 validate → unique_tag → insert pipeline directly into
//! the local SQLite primary store at `~/Library/Application Support/codo/
//! codo.db`. Drift-log entries for the typos / cosmetic noise / known
//! duplicate documented in docs/schema-extraction.md.
//!
//! ## Usage
//!
//! ```sh
//! cargo run --bin codo-migrate
//!     # default: docs/reference/2025 CI PRODUCTION V2.xlsb
//!
//! cargo run --bin codo-migrate -- /path/to/workbook.xlsb
//!
//! cargo run --bin codo-migrate -- /path/to/workbook.xlsb --dry-run
//!     # parse + canonicalize + validate, but don't write to the DB.
//!     # Useful for verifying counts before a real migration.
//! ```
//!
//! ## Idempotency
//!
//! Re-running is safe. The bin loads every existing `unique_tag` from the
//! local DB on startup and skips rows whose canonical tag is already
//! present. So running twice == running once + a "already imported, skipped"
//! line per row.
//!
//! ## Why a Rust CLI instead of the brain's planned Python script
//!
//! codo's canonicalize / §7.1 validate / unique_tag pipeline already lives
//! in Rust. A Python migrator would re-implement every rule and drift over
//! time. The Rust bin imports `codo_lib::commands::insert_production_event`
//! directly, so canonicalize logic has exactly ONE source of truth.

use calamine::{open_workbook_auto, Data, Range, Reader};
use chrono::NaiveDate;
use codo_lib::canonicalize::{
    canonicalize_disposition, canonicalize_grade, canonicalize_plant,
    canonicalize_source, canonicalize_warehouse, canonicalize_whse_side, Disposition, Grade,
    Plant, SourceCode, SourceKind, Warehouse,
};
use codo_lib::canonicalize::{Shift, Side};
use codo_lib::commands::{
    compute_unique_tag, insert_production_event_with_mode, CreateProductionEventInput,
    InsertMode, UniqueTagInput,
};
use codo_lib::validation::{validate_production_event, EventShape, ValidationError};
use codo_lib::db;
use libsql::{params, Connection};
use std::collections::HashSet;
use std::path::PathBuf;

const PRODUCTION_SHEET: &str = "Production";
const DVO_IN_SHEET: &str = "DVO IN";
const PC_WHSE_SHEETS: &[&str] = &["PC WHSE 1", "PC WHSE 2", "PC WHSE 5", "PC WHSE 7"];
const DEFAULT_XLSB: &str =
    "/Users/renzosy/CI-ICTC-inventory-app/docs/reference/2025 CI PRODUCTION V2.xlsb";

#[derive(Default, Debug)]
struct Stats {
    production_inserted: u64,
    production_skipped_idempotent: u64,
    production_skipped_legend: u64,
    production_failed_canon: u64,
    production_failed_validation: u64,
    production_unique_tag_collisions: u64,
    dvo_batches_created: u64,
    dvo_receipts_inserted: u64,
    opening_balances_inserted: u64,
    drift_entries: u64,
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut path: Option<PathBuf> = None;
    let mut dry_run = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--dry-run" => dry_run = true,
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            other => {
                if other.starts_with("--") {
                    eprintln!("unknown flag: {other}");
                    print_help();
                    std::process::exit(2);
                }
                path = Some(PathBuf::from(other));
            }
        }
    }
    let path = path.unwrap_or_else(|| PathBuf::from(DEFAULT_XLSB));
    if !path.exists() {
        eprintln!("✗ workbook not found: {}", path.display());
        eprintln!("  pass the path as the first argument");
        std::process::exit(1);
    }

    println!("▶ codo-migrate");
    println!("  workbook: {}", path.display());
    println!("  dry-run:  {dry_run}");

    // Open the local DB. open_local_db runs migrations + seeds idempotently.
    println!("▶ opening local DB at {}", db::local_db_path()?.display());
    let database = db::open_local_db().await?;
    let conn = database.connect()?;

    // Idempotency: pre-load every existing unique_tag so we can skip rows
    // already imported.
    let existing_tags = load_existing_unique_tags(&conn).await?;
    println!("  {} existing unique_tag(s) in local DB", existing_tags.len());

    // Open the workbook.
    let mut wb = open_workbook_auto(&path)?;

    let mut stats = Stats::default();

    // 1. Production sheet — the main 769 rows.
    println!("\n▶ migrating Production sheet");
    let prod_range = wb
        .worksheet_range(PRODUCTION_SHEET)
        .map_err(|e| format!("can't read sheet '{PRODUCTION_SHEET}': {e}"))?;
    migrate_production(&conn, &prod_range, &existing_tags, dry_run, &mut stats).await?;

    // 2. DVO IN sheet.
    println!("\n▶ migrating DVO IN sheet");
    match wb.worksheet_range(DVO_IN_SHEET) {
        Ok(range) => {
            migrate_dvo_in(&conn, &range, dry_run, &mut stats).await?;
        }
        Err(e) => {
            eprintln!("  (skipped, sheet '{DVO_IN_SHEET}' not readable: {e})");
        }
    }

    // 3. PC WHSE * STARTING blocks.
    println!("\n▶ migrating PC WHSE STARTING blocks → warehouse_opening_balance");
    for sheet in PC_WHSE_SHEETS {
        match wb.worksheet_range(sheet) {
            Ok(range) => {
                migrate_pc_whse_starting(&conn, sheet, &range, dry_run, &mut stats).await?;
            }
            Err(e) => {
                eprintln!("  (sheet '{sheet}' skipped: {e})");
            }
        }
    }

    // Summary.
    println!("\n========== Summary ==========");
    println!("Production sheet:");
    println!("  inserted              {}", stats.production_inserted);
    println!(
        "  skipped (already in)  {}",
        stats.production_skipped_idempotent
    );
    println!("  skipped (legend rows) {}", stats.production_skipped_legend);
    println!(
        "  failed (canonicalize) {}",
        stats.production_failed_canon
    );
    println!(
        "  failed (validation)   {}",
        stats.production_failed_validation
    );
    println!(
        "  unique_tag collisions {}",
        stats.production_unique_tag_collisions
    );
    println!("DVO IN sheet:");
    println!("  dvo_batches created   {}", stats.dvo_batches_created);
    println!(
        "  dvo_receipts inserted {}",
        stats.dvo_receipts_inserted
    );
    println!("PC WHSE STARTING:");
    println!(
        "  opening balances ins. {}",
        stats.opening_balances_inserted
    );
    println!("Drift log entries:      {}", stats.drift_entries);

    if dry_run {
        println!("\n(dry-run — nothing was written to the DB)");
    } else {
        println!("\n✓ migration complete. Click 'Sync now' in codo to push to the cloud backup.");
    }

    Ok(())
}

fn print_help() {
    println!(
        "codo-migrate — one-shot .xlsb → local SQLite migration\n\
         \n\
         USAGE:\n  \
           codo-migrate [PATH] [--dry-run]\n\
         \n\
         ARGS:\n  \
           PATH        path to 2025 CI PRODUCTION V2.xlsb (default: {DEFAULT_XLSB})\n  \
           --dry-run   parse + canonicalize + validate, do NOT write to DB\n  \
           -h, --help  this help\n"
    );
}

// ---------------------------------------------------------------------------
// Production sheet
// ---------------------------------------------------------------------------

async fn migrate_production(
    conn: &Connection,
    range: &Range<Data>,
    existing_tags: &HashSet<String>,
    dry_run: bool,
    stats: &mut Stats,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Per docs/schema-extraction.md §2:
    //   Row 1 (index 0) = headers
    //   Rows 2-6 (idx 1-5) = legend (sample valid values; UNIQUE TAG empty)
    //   Rows 7-11 (idx 6-10) = blank
    //   Row 12+ (idx 11+) = real data
    // We process every row but skip if UNIQUE TAG is empty (catches the
    // legend rows + any blanks regardless of position).

    let mut row_num = 0u64; // 1-indexed for human-friendly logs

    for row in range.rows() {
        row_num += 1;
        if row_num <= 1 {
            continue; // header
        }

        // The workbook has 16 columns + a trailing blank. Bail early if the
        // row is shorter than expected.
        if row.len() < 15 {
            continue;
        }

        // UNIQUE TAG empty → legend row or trailing blank → skip silently.
        let workbook_tag = cell_string(&row[14]);
        if workbook_tag.trim().is_empty() {
            stats.production_skipped_legend += 1;
            continue;
        }

        // Pull each cell as best we can.
        let recv_iso = cell_excel_date(&row[0]);
        let prod_iso = cell_excel_date(&row[1]);
        let batch = cell_string(&row[2]).trim().to_uppercase();
        let shift_raw = cell_string(&row[3]);
        let grade_raw = cell_string(&row[4]);
        let plant_raw = cell_string(&row[5]);
        let whse_raw = cell_string(&row[6]);
        let src_raw = cell_string(&row[7]);
        let weight = cell_f64(&row[8]).unwrap_or(0.0);
        let disposition_raw = cell_string(&row[9]);
        let flec_amt = cell_i64(&row[10]);
        let whse_side_raw = cell_string(&row[11]);
        let flec_stat = cell_optional_string(&row[12]);

        // Canonicalize. Each step that fails on garbage logs to drift_log
        // and either substitutes NULL (where the column is nullable) or
        // skips the row (where the column is required).

        let recv_iso = match recv_iso {
            Some(s) => s,
            None => {
                stats.production_failed_canon += 1;
                stats.drift_entries += 1;
                if !dry_run {
                    log_drift(
                        conn,
                        "production_recv_date_unparseable",
                        None,
                        None,
                        Some(&format!("{:?}", row[0])),
                        Some(&format!("workbook row {row_num}")),
                    )
                    .await?;
                }
                continue;
            }
        };

        let grade = match canonicalize_grade(&grade_raw) {
            Ok(g) => g,
            Err(_) => {
                stats.production_failed_canon += 1;
                stats.drift_entries += 1;
                if !dry_run {
                    log_drift(
                        conn,
                        "production_grade_unknown",
                        None,
                        None,
                        Some(&grade_raw),
                        Some(&format!("workbook row {row_num} — required field, row skipped")),
                    )
                    .await?;
                }
                continue;
            }
        };

        let source = match canonicalize_source(&src_raw) {
            Ok(s) => s,
            Err(_) => {
                stats.production_failed_canon += 1;
                stats.drift_entries += 1;
                if !dry_run {
                    log_drift(
                        conn,
                        "production_source_unknown",
                        None,
                        None,
                        Some(&src_raw),
                        Some(&format!("workbook row {row_num} — required field, row skipped")),
                    )
                    .await?;
                }
                continue;
            }
        };

        // Plant: §7.2 forces plant from source for non-FLEC sources. Only
        // honour the workbook's raw plant when source is FLEC. Drift-log
        // garbage values either way.
        let raw_plant_canon = match canonicalize_plant(&plant_raw) {
            Ok(p) => Some(p),
            Err(_) => {
                if !plant_raw.trim().is_empty() {
                    stats.drift_entries += 1;
                    if !dry_run {
                        log_drift(
                            conn,
                            "production_plant_garbage",
                            None,
                            None,
                            Some(&plant_raw),
                            Some(&format!(
                                "workbook row {row_num} — substituting source-derived plant"
                            )),
                        )
                        .await?;
                    }
                }
                None
            }
        };

        // Warehouse: §6.7 / §2 cosmetic noise W6/W7 → NULL with drift entry.
        let warehouse = match canonicalize_warehouse(&whse_raw) {
            Ok(opt) => opt,
            Err(_) => {
                stats.drift_entries += 1;
                if !dry_run {
                    log_drift(
                        conn,
                        "production_warehouse_unknown",
                        None,
                        None,
                        Some(&whse_raw),
                        Some(&format!("workbook row {row_num} — substituting NULL")),
                    )
                    .await?;
                }
                None
            }
        };

        // If WHSE column had cosmetic W6/W7, drift-log that too. The
        // canonicalize layer already returned Ok(None) without erroring,
        // so we detect it here.
        let trimmed_whse_upper = whse_raw.trim().to_uppercase();
        if trimmed_whse_upper == "W6" || trimmed_whse_upper == "W7" || trimmed_whse_upper == "W3" {
            stats.drift_entries += 1;
            if !dry_run {
                log_drift(
                    conn,
                    "whse_w6_w7_cosmetic",
                    None,
                    None,
                    Some(&whse_raw),
                    Some(&format!("workbook row {row_num}")),
                )
                .await?;
            }
        }

        // WHSE SIDE: either LS/RS for WHSE 1/2/5/7 or a DVO batch code for
        // WHSE 3. Try canonicalize_whse_side first; if it's not LS/RS, try
        // parsing as a DVO batch code.
        let whse_side_str = whse_side_raw.trim();
        let (final_whse_side, dvo_batch_code) = if whse_side_str.is_empty() {
            (None, None)
        } else {
            match canonicalize_whse_side(whse_side_str) {
                Ok(opt) => (opt, None),
                Err(_) => {
                    // Maybe a DVO batch code like 'NOVEMBER2025RIGHT'.
                    if parse_dvo_batch_code(whse_side_str).is_some() {
                        (None, Some(whse_side_str.to_uppercase()))
                    } else {
                        stats.drift_entries += 1;
                        if !dry_run {
                            log_drift(
                                conn,
                                "production_whse_side_unknown",
                                None,
                                None,
                                Some(whse_side_str),
                                Some(&format!("workbook row {row_num}")),
                            )
                            .await?;
                        }
                        (None, None)
                    }
                }
            }
        };

        // Resolve dvo_batch_id from the code if present.
        let dvo_batch_id = if let Some(code) = &dvo_batch_code {
            Some(ensure_dvo_batch(conn, code, dry_run, stats).await?)
        } else {
            None
        };

        // Disposition.
        let disposition = match canonicalize_disposition(&disposition_raw) {
            Ok(d) => d,
            Err(_) => {
                stats.production_failed_canon += 1;
                stats.drift_entries += 1;
                if !dry_run {
                    log_drift(
                        conn,
                        "production_disposition_unknown",
                        None,
                        None,
                        Some(&disposition_raw),
                        Some(&format!("workbook row {row_num} — required field, row skipped")),
                    )
                    .await?;
                }
                continue;
            }
        };

        // Plant resolution per §7.2 — forced from source for non-FLEC
        // sources; only honour the workbook's raw plant when source is FLEC.
        let derived_plant = match (raw_plant_canon, source.kind()) {
            (_, SourceKind::Tank | SourceKind::PlantDirect | SourceKind::DvoContainer) => {
                match source {
                    SourceCode::Tnk1 | SourceCode::Tnk2 | SourceCode::Tnk3 | SourceCode::Tnk4
                    | SourceCode::W6 => Some(Plant::W6),
                    SourceCode::W7 => Some(Plant::W7),
                    SourceCode::Dvo => Some(Plant::Dvo),
                    _ => None,
                }
            }
            (Some(p), SourceKind::WarehouseFlec) => Some(p),
            (None, SourceKind::WarehouseFlec) => None,
        };

        // Canonicalize shift + whse_side for unique_tag computation.
        let canon_shift: Option<Shift> = match shift_raw.trim() {
            "" => None,
            s => codo_lib::canonicalize::canonicalize_shift(s).ok(),
        };
        let canon_whse_side_for_tag: Option<Side> = match canonicalize_whse_side(whse_side_raw.trim()) {
            Ok(opt) => opt,
            Err(_) => None,
        };

        // Idempotency: compute the canonical unique_tag the same way
        // insert_production_event will. Exact pre-check — re-runs skip
        // every previously-imported row instead of hitting the DB UNIQUE.
        let recv_naive = NaiveDate::parse_from_str(&recv_iso, "%Y-%m-%d").ok();
        let prod_naive = prod_iso
            .as_ref()
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
        if let Some(recv) = recv_naive {
            let canonical_tag = compute_unique_tag(&UniqueTagInput {
                recv_date: recv,
                prod_date: prod_naive,
                batch: &batch,
                shift: canon_shift.as_ref(),
                grade,
                plant: derived_plant.as_ref(),
                warehouse: warehouse.as_ref(),
                whse_side: canon_whse_side_for_tag.as_ref(),
                source,
                disposition,
            });
            if existing_tags.contains(&canonical_tag) {
                stats.production_skipped_idempotent += 1;
                continue;
            }
        }

        // Build the input. We pass raw strings; insert_production_event
        // will canonicalize again (idempotent) and skip §7.1 validation
        // when called with InsertMode::Migration.
        let plant_override = match (source.kind(), raw_plant_canon) {
            (SourceKind::WarehouseFlec, Some(p)) => Some(p.code().to_string()),
            _ => None, // forced from source per §7.2 OR none/garbage
        };

        let input = CreateProductionEventInput {
            recv_date: recv_iso.clone(),
            prod_date: prod_iso.clone(),
            batch: batch.clone(),
            shift_code: if shift_raw.trim().is_empty() {
                None
            } else {
                Some(shift_raw.clone())
            },
            grade_code: grade.code().to_string(),
            source_code: source.code().to_string(),
            plant_code_override: plant_override,
            warehouse_code: warehouse.map(|w| w.code().to_string()),
            disposition_raw: disposition_raw.clone(),
            weight_kg: weight,
            flec_count: flec_amt,
            whse_side: final_whse_side.map(|s| s.code().to_string()),
            flec_stat,
            dvo_batch_id,
            notes: None,
        };
        if let Err(ve) = validate_production_event(EventShape {
            disposition,
            source,
            warehouse,
            plant: derived_plant,
        }) {
            stats.drift_entries += 1;
            if !dry_run {
                let kind = match &ve {
                    ValidationError::PlantMismatch { .. } => "production_plant_mismatch_legacy",
                    ValidationError::Forbidden { .. } => "production_71_violation_legacy",
                    ValidationError::MissingField(_) => "production_missing_field_legacy",
                };
                log_drift(
                    conn,
                    kind,
                    None,
                    None,
                    Some(&format!("{ve}")),
                    Some(&format!(
                        "workbook row {row_num} — imported under InsertMode::Migration; review"
                    )),
                )
                .await?;
            }
        }

        if dry_run {
            stats.production_inserted += 1;
            if stats.production_inserted % 100 == 0 {
                println!(
                    "  [{:4}/   ?] dry-run ok — workbook_tag={workbook_tag}",
                    stats.production_inserted
                );
            }
            continue;
        }

        match insert_production_event_with_mode(conn, &input, InsertMode::Migration).await {
            Ok(row) => {
                stats.production_inserted += 1;
                if stats.production_inserted % 100 == 0 {
                    println!(
                        "  [{:4}/   ?] inserted unique_tag={}",
                        stats.production_inserted, row.unique_tag
                    );
                }
            }
            Err(e) => {
                let msg = format!("{e}");
                if msg.to_lowercase().contains("unique") {
                    stats.production_unique_tag_collisions += 1;
                    stats.drift_entries += 1;
                    log_drift(
                        conn,
                        "unique_tag_collision",
                        None,
                        None,
                        Some(&workbook_tag),
                        Some(&format!("workbook row {row_num} — confirmed-mistake duplicate")),
                    )
                    .await?;
                } else if msg.to_lowercase().contains("forbidden")
                    || msg.to_lowercase().contains("plant_id")
                {
                    stats.production_failed_validation += 1;
                    stats.drift_entries += 1;
                    log_drift(
                        conn,
                        "production_validation_failed",
                        None,
                        None,
                        Some(&msg),
                        Some(&format!("workbook row {row_num}")),
                    )
                    .await?;
                } else {
                    eprintln!("  ✗ row {row_num} failed: {e}");
                    stats.production_failed_validation += 1;
                    stats.drift_entries += 1;
                    log_drift(
                        conn,
                        "production_insert_error_other",
                        None,
                        None,
                        Some(&msg),
                        Some(&format!("workbook row {row_num} — uncategorized insert error")),
                    )
                    .await?;
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// DVO IN sheet
// ---------------------------------------------------------------------------
//
// docs/schema-extraction.md §6.6 documents the column set loosely. The
// actual layout in the workbook is inspected at run-time — we read the
// header row and map by header name. Any column we can't find gets a
// drift entry and we skip rows that lack the essentials.
//
// Required columns: GOTHONG TRACKING SLIPS, ACTUAL WEIGHT (Cebu scale),
// DATE RECV, BATCH/SIDE. The Davao-side declared weight column has
// shifted names across versions; we accept several variants.

async fn migrate_dvo_in(
    conn: &Connection,
    range: &Range<Data>,
    dry_run: bool,
    stats: &mut Stats,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Find the header row. The DVO IN sheet has some title/legend rows
    // above the data; we scan for the row containing the GOTHONG header.
    let mut header_row_idx: Option<usize> = None;
    let mut col_idx: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (i, row) in range.rows().enumerate() {
        let normalized: Vec<String> = row.iter().map(|c| cell_string(c).trim().to_uppercase()).collect();
        if normalized.iter().any(|s| s.contains("GOTHONG")) {
            header_row_idx = Some(i);
            for (j, h) in normalized.iter().enumerate() {
                if !h.is_empty() {
                    col_idx.insert(h.clone(), j);
                }
            }
            break;
        }
    }
    let Some(start) = header_row_idx else {
        eprintln!("  (no GOTHONG header found in DVO IN sheet, skipping)");
        return Ok(());
    };

    let col_gothong = col_idx
        .iter()
        .find(|(k, _)| k.contains("GOTHONG"))
        .map(|(_, v)| *v);
    let col_dvo_declared = col_idx
        .iter()
        .find(|(k, _)| k.contains("DVO") && (k.contains("DECLARED") || k.contains("WEIGHT")))
        .or_else(|| col_idx.iter().find(|(k, _)| k.contains("BOL") || k.contains("DECLARED")))
        .map(|(_, v)| *v);
    let col_actual = col_idx
        .iter()
        .find(|(k, _)| k.contains("ACTUAL") && k.contains("WEIGHT"))
        .map(|(_, v)| *v);
    let col_recv = col_idx
        .iter()
        .find(|(k, _)| k.contains("DATE") && k.contains("RECV"))
        .or_else(|| col_idx.iter().find(|(k, _)| k.starts_with("DATE")))
        .map(|(_, v)| *v);
    let col_batch_side = col_idx
        .iter()
        .find(|(k, _)| k.contains("BATCH") && k.contains("SIDE"))
        .or_else(|| col_idx.iter().find(|(k, _)| k.contains("BATCH/SIDE")))
        .map(|(_, v)| *v);
    let col_sks = col_idx
        .iter()
        .find(|(k, _)| k.trim() == "SKS" || k.contains("SACK"))
        .map(|(_, v)| *v);
    let col_at_cy = col_idx
        .iter()
        .find(|(k, _)| k.contains("PULLOUT") || k.contains("AT CEBU"))
        .map(|(_, v)| *v);

    let _ = col_gothong; // optional fields we don't strictly need for insert
    let _ = col_at_cy;

    let needed = [
        ("ACTUAL WEIGHT", col_actual),
        ("DATE RECV", col_recv),
        ("BATCH/SIDE", col_batch_side),
    ];
    for (name, opt) in needed {
        if opt.is_none() {
            eprintln!("  (column '{name}' not found in DVO IN — skipping sheet)");
            return Ok(());
        }
    }
    let col_actual = col_actual.unwrap();
    let col_recv = col_recv.unwrap();
    let col_batch_side = col_batch_side.unwrap();

    for row in range.rows().skip(start + 1) {
        if row.is_empty() {
            continue;
        }

        let actual = match col_actual.checked_sub(0).and_then(|i| row.get(i)).and_then(cell_f64) {
            Some(v) if v > 0.0 => v,
            _ => continue, // empty / blank row
        };
        let recv_iso = match row.get(col_recv).and_then(cell_excel_date) {
            Some(s) => s,
            None => continue,
        };
        let batch_code = row
            .get(col_batch_side)
            .map(cell_string)
            .map(|s| s.trim().to_uppercase())
            .unwrap_or_default();
        if batch_code.is_empty() {
            continue;
        }

        // Davao declared weight: optional; default to actual if not present.
        let dvo_declared = col_dvo_declared
            .and_then(|i| row.get(i))
            .and_then(cell_f64)
            .filter(|v| *v > 0.0)
            .unwrap_or(actual);

        let gothong = col_gothong.and_then(|i| row.get(i)).map(cell_optional_string).flatten();
        let sks = col_sks.and_then(|i| row.get(i)).and_then(cell_i64);
        let at_cy = col_at_cy
            .and_then(|i| row.get(i))
            .map(cell_string)
            .map(|s| {
                let u = s.trim().to_uppercase();
                u == "Y" || u == "YES" || u == "TRUE" || u == "1"
            })
            .unwrap_or(false);

        if dry_run {
            stats.dvo_receipts_inserted += 1;
            continue;
        }

        let dvo_batch_id = ensure_dvo_batch(conn, &batch_code, false, stats).await?;

        // Idempotency for dvo_receipt: dvo_receipt has no UNIQUE constraint
        // in v1 schema, so we pre-check on the natural key (batch_id + recv_date
        // + gothong_slip if present, else also weights). Matches treat the
        // existing row as authoritative; the migration is a no-op for it.
        let already_present = check_dvo_receipt_exists(
            conn,
            dvo_batch_id,
            &recv_iso,
            gothong.as_deref(),
            dvo_declared,
            actual,
        )
        .await?;
        if already_present {
            continue;
        }

        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO dvo_receipt (dvo_batch_id, recv_date, gothong_slip, sack_count, dvo_declared_weight_kg, cebu_declared_weight_kg, at_cebu_cy, notes, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)",
            params![
                dvo_batch_id,
                recv_iso,
                gothong,
                sks,
                dvo_declared,
                actual,
                if at_cy { 1_i64 } else { 0_i64 },
                now.clone(),
                now,
            ],
        )
        .await?;
        stats.dvo_receipts_inserted += 1;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// PC WHSE 1/2/5/7 STARTING blocks → warehouse_opening_balance
// ---------------------------------------------------------------------------
//
// Per docs/schema-extraction.md §4.1, each PC WHSE * sheet has the layout:
//   B1: "START:"   C1: <date>      ← the period_start_date
//   B2: "WHSE:"    C2: <code>      ← redundant w/ sheet name; trust C2
//   ...
//   B6:B12 = grade codes
//   E6:E12 = STARTING RS values (user-typed)
//   F6:F12 = STARTING LS values (user-typed)
// We process each non-zero (grade, side) pair as one opening_balance row.

async fn migrate_pc_whse_starting(
    conn: &Connection,
    sheet_name: &str,
    range: &Range<Data>,
    dry_run: bool,
    stats: &mut Stats,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    // C1 (row index 0, col 2) — period start date.
    let period_start = range
        .get((0, 2))
        .and_then(cell_excel_date)
        .unwrap_or_else(|| chrono::Utc::now().date_naive().format("%Y-%m-%d").to_string());

    // C2 (row index 1, col 2) — warehouse code. Fallback to sheet name.
    let warehouse_code = range
        .get((1, 2))
        .map(cell_string)
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .or_else(|| Some(sheet_name.replace("PC ", "")))
        .unwrap();

    let warehouse = match canonicalize_warehouse(&warehouse_code) {
        Ok(Some(w)) => w,
        _ => {
            eprintln!("  (sheet '{sheet_name}' warehouse '{warehouse_code}' unrecognized — skipping)");
            return Ok(());
        }
    };
    let warehouse_id = warehouse_id_for(warehouse);

    // Rows 6-12 (idx 5-11) carry one grade per row.
    for row_idx in 5..=11 {
        let grade_raw = range.get((row_idx, 1)).map(cell_string).unwrap_or_default();
        if grade_raw.trim().is_empty() {
            continue;
        }
        let grade = match canonicalize_grade(&grade_raw) {
            Ok(g) => g,
            Err(_) => continue,
        };
        let grade_id = grade_id_for(grade);

        for (col_idx, side_code) in [(4usize, "RS"), (5usize, "LS")] {
            let val = range
                .get((row_idx, col_idx))
                .and_then(cell_i64)
                .or_else(|| {
                    range
                        .get((row_idx, col_idx))
                        .and_then(cell_f64)
                        .map(|f| f as i64)
                })
                .unwrap_or(0);
            if val <= 0 {
                continue;
            }
            if dry_run {
                stats.opening_balances_inserted += 1;
                continue;
            }
            let now = chrono::Utc::now().to_rfc3339();
            let res = conn.execute(
                "INSERT OR IGNORE INTO warehouse_opening_balance (warehouse_id, grade_id, side, period_start_date, opening_flec_count, notes, created_at, updated_at) VALUES (?, ?, ?, ?, ?, NULL, ?, ?)",
                params![
                    warehouse_id,
                    grade_id,
                    side_code,
                    period_start.clone(),
                    val,
                    now.clone(),
                    now,
                ],
            ).await?;
            if res > 0 {
                stats.opening_balances_inserted += 1;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn load_existing_unique_tags(
    conn: &Connection,
) -> std::result::Result<HashSet<String>, Box<dyn std::error::Error>> {
    let mut tags = HashSet::new();
    let mut rows = conn
        .query("SELECT unique_tag FROM production_event", params![])
        .await?;
    while let Some(r) = rows.next().await? {
        tags.insert(r.get::<String>(0)?);
    }
    Ok(tags)
}

async fn check_dvo_receipt_exists(
    conn: &Connection,
    dvo_batch_id: i64,
    recv_iso: &str,
    gothong: Option<&str>,
    dvo_declared: f64,
    cebu_declared: f64,
) -> std::result::Result<bool, Box<dyn std::error::Error>> {
    // Match on (batch, recv_date, gothong) when gothong is present (the
    // strongest natural key — Gothong tracking slips are unique per
    // shipment). Fall back to a (batch, recv_date, weights) match when
    // gothong is missing.
    let mut rows = if let Some(slip) = gothong {
        conn.query(
            "SELECT 1 FROM dvo_receipt WHERE dvo_batch_id = ? AND recv_date = ? AND gothong_slip = ? LIMIT 1",
            params![dvo_batch_id, recv_iso, slip],
        )
        .await?
    } else {
        conn.query(
            "SELECT 1 FROM dvo_receipt WHERE dvo_batch_id = ? AND recv_date = ? AND ABS(dvo_declared_weight_kg - ?) < 0.01 AND ABS(cebu_declared_weight_kg - ?) < 0.01 AND gothong_slip IS NULL LIMIT 1",
            params![dvo_batch_id, recv_iso, dvo_declared, cebu_declared],
        )
        .await?
    };
    Ok(rows.next().await?.is_some())
}

async fn ensure_dvo_batch(
    conn: &Connection,
    code: &str,
    dry_run: bool,
    stats: &mut Stats,
) -> std::result::Result<i64, Box<dyn std::error::Error>> {
    if dry_run {
        return Ok(0);
    }
    // Existing?
    let mut rows = conn
        .query("SELECT id FROM dvo_batch WHERE code = ?", params![code])
        .await?;
    if let Some(r) = rows.next().await? {
        return Ok(r.get(0)?);
    }
    drop(rows);

    // Parse code into (start_month, year, side). If unparseable, default
    // start_month=1 year=current side=LEFT and drift-log.
    let (start_month, year, side) = parse_dvo_batch_code(code).unwrap_or_else(|| {
        (1, chrono::Utc::now().date_naive().format("%Y").to_string().parse().unwrap_or(2026), "LEFT".to_string())
    });
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO dvo_batch (code, warehouse_id, start_month, year, side, status, opened_at, created_at, updated_at) VALUES (?, 3, ?, ?, ?, 'open', ?, ?, ?)",
        params![code, start_month as i64, year as i64, side, now.clone(), now.clone(), now],
    )
    .await?;
    stats.dvo_batches_created += 1;

    let mut rows = conn
        .query("SELECT id FROM dvo_batch WHERE code = ?", params![code])
        .await?;
    let r = rows
        .next()
        .await?
        .ok_or("just-inserted dvo_batch row not found")?;
    Ok(r.get(0)?)
}

/// Parse 'NOVEMBER2025RIGHT' / 'SEPTEMBER2025LEFT' → (start_month, year, "LEFT"|"RIGHT").
fn parse_dvo_batch_code(code: &str) -> Option<(u8, u16, String)> {
    let upper = code.trim().to_uppercase();
    let months = [
        "JANUARY",
        "FEBRUARY",
        "MARCH",
        "APRIL",
        "MAY",
        "JUNE",
        "JULY",
        "AUGUST",
        "SEPTEMBER",
        "OCTOBER",
        "NOVEMBER",
        "DECEMBER",
    ];
    let mut found: Option<(usize, u8)> = None;
    for (i, m) in months.iter().enumerate() {
        if upper.starts_with(m) {
            found = Some((m.len(), (i + 1) as u8));
            break;
        }
    }
    let (month_end, start_month) = found?;
    let rest = &upper[month_end..];
    // Year is 4 digits.
    if rest.len() < 5 {
        return None;
    }
    let year: u16 = rest[0..4].parse().ok()?;
    let side_raw = &rest[4..];
    let side = if side_raw == "LEFT" {
        "LEFT"
    } else if side_raw == "RIGHT" {
        "RIGHT"
    } else {
        return None;
    };
    Some((start_month, year, side.to_string()))
}

async fn log_drift(
    conn: &Connection,
    kind: &str,
    target_id: Option<i64>,
    expected: Option<&str>,
    actual: Option<&str>,
    message: Option<&str>,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    conn.execute(
        "INSERT INTO drift_log (detected_at, kind, target_id, expected, actual, message) VALUES (?, ?, ?, ?, ?, ?)",
        params![
            chrono::Utc::now().to_rfc3339(),
            kind,
            target_id,
            expected.unwrap_or(""),
            actual.unwrap_or(""),
            message.unwrap_or(""),
        ],
    )
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Cell extractors
// ---------------------------------------------------------------------------

fn cell_string(c: &Data) -> String {
    match c {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                format!("{f}")
            }
        }
        Data::Int(i) => format!("{i}"),
        Data::Bool(b) => format!("{b}"),
        Data::DateTime(d) => format!("{}", d.as_f64()),
        Data::DateTimeIso(s) => s.clone(),
        Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("ERR:{e:?}"),
    }
}

fn cell_optional_string(c: &Data) -> Option<String> {
    match c {
        Data::Empty => None,
        _ => {
            let s = cell_string(c);
            if s.trim().is_empty() {
                None
            } else {
                Some(s)
            }
        }
    }
}

fn cell_f64(c: &Data) -> Option<f64> {
    match c {
        Data::Float(f) => Some(*f),
        Data::Int(i) => Some(*i as f64),
        Data::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn cell_i64(c: &Data) -> Option<i64> {
    match c {
        Data::Int(i) => Some(*i),
        Data::Float(f) if f.fract() == 0.0 => Some(*f as i64),
        Data::String(s) => s.parse::<i64>().ok(),
        _ => None,
    }
}

/// Excel date serial → ISO 'YYYY-MM-DD'.
fn cell_excel_date(c: &Data) -> Option<String> {
    let serial = match c {
        Data::Float(f) => *f,
        Data::Int(i) => *i as f64,
        Data::DateTime(d) => d.as_f64(),
        Data::DateTimeIso(s) => return Some(s.clone()),
        _ => return None,
    };
    excel_serial_to_iso(serial)
}

fn excel_serial_to_iso(serial: f64) -> Option<String> {
    let days = serial.trunc() as i64;
    let epoch = NaiveDate::from_ymd_opt(1899, 12, 30)?;
    let date = epoch.checked_add_days(chrono::Days::new(days as u64))?;
    Some(date.format("%Y-%m-%d").to_string())
}

// ---------------------------------------------------------------------------
// id helpers — match commands.rs's deterministic seeded IDs.
// ---------------------------------------------------------------------------

fn warehouse_id_for(w: Warehouse) -> i64 {
    match w {
        Warehouse::W1 => 1,
        Warehouse::W2 => 2,
        Warehouse::W3 => 3,
        Warehouse::W5 => 5,
        Warehouse::W7 => 7,
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

// Unused but compiled — these mirror commands.rs id maps for future use
// when the migration grows to need plant/source ids directly.
#[allow(dead_code)]
fn plant_id_for(p: Plant) -> i64 {
    match p {
        Plant::W6 => 1,
        Plant::W7 => 2,
        Plant::W6W7 => 3,
        Plant::Dvo => 4,
    }
}
#[allow(dead_code)]
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
#[allow(dead_code)]
fn disposition_kind_code(d: Disposition) -> &'static str {
    d.kind_code()
}
