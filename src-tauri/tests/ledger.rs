//! Integration tests for the warehouse_ledger_flec + dvo_batch_ledger
//! function bodies (Step 5a + 5b).
//!
//! These tests build a fresh tmpdir libSQL DB, seed minimal lookup tables
//! via the existing migrations, then INSERT a few production_event /
//! warehouse_opening_balance / dvo_batch / dvo_receipt rows by hand and
//! assert the ledger functions return exactly the running balances we'd
//! work out on paper.
//!
//! The brain calls out one specific sample to check:
//!   `PC WHSE 7` STARTING (3X50, RS) = 53. First in-row is FLEC IN = 33.
//!   RUN BAL on that row should be 86. Locked in below.

use chrono::NaiveDate;
use codo_lib::canonicalize::{Grade, Side, Warehouse};
use codo_lib::commands::{
    insert_production_event_with_mode, CreateProductionEventInput, InsertMode,
};
use codo_lib::db;
use codo_lib::ledger::{dvo_batch_ledger, warehouse_ledger_flec, DvoBatchStatus, DvoLedgerEvent};
use libsql::params;

async fn fresh_db() -> (tempfile::TempDir, libsql::Database, libsql::Connection) {
    let tmp = tempfile::tempdir().expect("tmpdir");
    let path = tmp.path().join("codo-test.db");
    let database = libsql::Builder::new_local(path)
        .build()
        .await
        .expect("build local libsql");
    let conn = database.connect().expect("connect");
    db::run_migrations(&conn).await.expect("run migrations");
    (tmp, database, conn)
}

async fn insert_opening(
    conn: &libsql::Connection,
    warehouse_id: i64,
    grade_id: i64,
    side: &str,
    date: &str,
    flec: i64,
) {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO warehouse_opening_balance (warehouse_id, grade_id, side, period_start_date, opening_flec_count, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![warehouse_id, grade_id, side, date, flec, now.clone(), now],
    )
    .await
    .unwrap();
}

fn input(
    recv_date: &str,
    grade: &str,
    source: &str,
    warehouse: Option<&str>,
    side: Option<&str>,
    weight: f64,
    flec: Option<i64>,
    disposition: &str,
) -> CreateProductionEventInput {
    CreateProductionEventInput {
        recv_date: recv_date.to_string(),
        prod_date: Some(recv_date.to_string()),
        batch: "MAY".to_string(),
        shift_code: Some("M".to_string()),
        grade_code: grade.to_string(),
        source_code: source.to_string(),
        plant_code_override: None,
        warehouse_code: warehouse.map(|s| s.to_string()),
        disposition_raw: disposition.to_string(),
        weight_kg: weight,
        flec_count: flec,
        whse_side: side.map(|s| s.to_string()),
        flec_stat: None,
        dvo_batch_id: None,
        notes: None,
    }
}

// ---------------------------------------------------------------------------
// warehouse_ledger_flec — happy path locking in the brain §4.5 example.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn flec_ledger_starts_at_opening_balance() {
    let (_tmp, _db, conn) = fresh_db().await;
    insert_opening(&conn, 7, 1, "RS", "2026-03-01", 53).await;

    let ledger = warehouse_ledger_flec(
        &conn,
        Warehouse::W7,
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(ledger.opening_balances.len(), 1);
    assert_eq!(ledger.opening_balances[0].grade, Grade::G3x50);
    assert_eq!(ledger.opening_balances[0].side, Side::Rs);
    assert_eq!(ledger.opening_balances[0].flec_count, 53);

    assert!(ledger.rows.is_empty());

    assert_eq!(ledger.current_balances.len(), 1);
    let cb = &ledger.current_balances[0];
    assert_eq!(cb.grade, Grade::G3x50);
    assert_eq!(cb.side, Side::Rs);
    assert_eq!(cb.flec_count, 53);
    assert_eq!(cb.components.opening, 53);
    assert_eq!(cb.components.flec_in_to_date, 0);
    assert_eq!(cb.components.flec_out_to_date, 0);
}

#[tokio::test]
async fn flec_ledger_brain_section_4_5_example() {
    // Brain §4.5 example: STARTING (3X50, RS) = 53. First in-row is FLEC IN
    // = 33. RUN BAL on that row should be 86. Lock that in.
    let (_tmp, _db, conn) = fresh_db().await;
    insert_opening(&conn, 7, 1, "RS", "2026-03-10", 53).await;

    insert_production_event_with_mode(
        &conn,
        &input(
            "2026-03-15",
            "3X50",
            "TNK 1",
            Some("WHSE 7"),
            Some("RS"),
            14000.0,
            Some(33),
            "FLEC",
        ),
        InsertMode::Strict,
    )
    .await
    .unwrap();

    let ledger = warehouse_ledger_flec(
        &conn,
        Warehouse::W7,
        NaiveDate::from_ymd_opt(2026, 3, 10).unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(ledger.rows.len(), 1);
    let row = &ledger.rows[0];
    assert_eq!(row.grade, Grade::G3x50);
    assert_eq!(row.side, Some(Side::Rs));
    assert_eq!(row.flec_in, Some(33));
    assert_eq!(row.flec_out, None);
    assert_eq!(row.run_bal_flec, 86);
    assert_eq!(row.run_bal_components.opening, 53);
    assert_eq!(row.run_bal_components.flec_in_to_date, 33);
    assert_eq!(row.run_bal_components.flec_out_to_date, 0);

    let cb = &ledger.current_balances[0];
    assert_eq!(cb.flec_count, 86);
}

#[tokio::test]
async fn flec_ledger_handles_multiple_rows_same_grade_side() {
    let (_tmp, _db, conn) = fresh_db().await;
    insert_opening(&conn, 7, 1, "RS", "2026-03-01", 50).await;

    // Three FLEC inflows to (3X50, RS): +20, +15, +10 → balance 95.
    for (date, flec) in [("2026-03-05", 20), ("2026-03-06", 15), ("2026-03-07", 10)] {
        insert_production_event_with_mode(
            &conn,
            &input(
                date,
                "3X50",
                "TNK 1",
                Some("WHSE 7"),
                Some("RS"),
                10000.0,
                Some(flec),
                "FLEC",
            ),
            InsertMode::Strict,
        )
        .await
        .unwrap();
    }

    let ledger = warehouse_ledger_flec(
        &conn,
        Warehouse::W7,
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(ledger.rows.len(), 3);
    assert_eq!(ledger.rows[0].run_bal_flec, 70); // 50 + 20
    assert_eq!(ledger.rows[1].run_bal_flec, 85); // 70 + 15
    assert_eq!(ledger.rows[2].run_bal_flec, 95); // 85 + 10

    assert_eq!(ledger.current_balances.len(), 1);
    assert_eq!(ledger.current_balances[0].flec_count, 95);
}

#[tokio::test]
async fn flec_ledger_inflow_then_partner_outflow() {
    let (_tmp, _db, conn) = fresh_db().await;
    insert_opening(&conn, 7, 1, "LS", "2026-03-01", 50).await;

    // FLEC bagging: +30 → 80
    insert_production_event_with_mode(
        &conn,
        &input(
            "2026-03-05",
            "3X50",
            "TNK 2",
            Some("WHSE 7"),
            Some("LS"),
            14000.0,
            Some(30),
            "FLEC",
        ),
        InsertMode::Strict,
    )
    .await
    .unwrap();
    // Partner crusher takes 12 from the warehouse: −12 → 68
    insert_production_event_with_mode(
        &conn,
        &input(
            "2026-03-06",
            "3X50",
            "FLEC",
            Some("WHSE 7"),
            Some("LS"),
            5500.0,
            Some(12),
            "C1",
        ),
        InsertMode::Strict,
    )
    .await
    .unwrap();

    let ledger = warehouse_ledger_flec(
        &conn,
        Warehouse::W7,
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(ledger.rows.len(), 2);
    assert_eq!(ledger.rows[0].flec_in, Some(30));
    assert_eq!(ledger.rows[0].run_bal_flec, 80);
    assert_eq!(ledger.rows[1].flec_out, Some(12));
    assert_eq!(ledger.rows[1].run_bal_flec, 68);
    assert_eq!(ledger.current_balances[0].flec_count, 68);
}

#[tokio::test]
async fn flec_ledger_keeps_grade_side_separate() {
    // (3X50, RS) and (2X6, LS) are tracked independently.
    let (_tmp, _db, conn) = fresh_db().await;
    insert_opening(&conn, 7, 1, "RS", "2026-03-01", 53).await; // 3X50, RS
    insert_opening(&conn, 7, 2, "LS", "2026-03-01", 26).await; // 2X6,  LS

    insert_production_event_with_mode(
        &conn,
        &input(
            "2026-03-05",
            "3X50",
            "TNK 1",
            Some("WHSE 7"),
            Some("RS"),
            14000.0,
            Some(33),
            "FLEC",
        ),
        InsertMode::Strict,
    )
    .await
    .unwrap();
    insert_production_event_with_mode(
        &conn,
        &input(
            "2026-03-06",
            "2X6",
            "W7",
            Some("WHSE 7"),
            Some("LS"),
            10000.0,
            Some(20),
            "FLEC",
        ),
        InsertMode::Strict,
    )
    .await
    .unwrap();

    let ledger = warehouse_ledger_flec(
        &conn,
        Warehouse::W7,
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(ledger.rows.len(), 2);
    assert_eq!(ledger.rows[0].run_bal_flec, 86); // 53 + 33
    assert_eq!(ledger.rows[1].run_bal_flec, 46); // 26 + 20

    assert_eq!(ledger.current_balances.len(), 2);
    let by_pair: std::collections::HashMap<(Grade, Side), i64> = ledger
        .current_balances
        .iter()
        .map(|c| ((c.grade, c.side), c.flec_count))
        .collect();
    assert_eq!(by_pair[&(Grade::G3x50, Side::Rs)], 86);
    assert_eq!(by_pair[&(Grade::G2x6, Side::Ls)], 46);
}

#[tokio::test]
async fn flec_ledger_rejects_whse_3() {
    let (_tmp, _db, conn) = fresh_db().await;
    let err = warehouse_ledger_flec(
        &conn,
        Warehouse::W3,
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
    )
    .await
    .unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.to_lowercase().contains("dvo_batch_ledger"),
        "expected hint about dvo_batch_ledger, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// dvo_batch_ledger
// ---------------------------------------------------------------------------

async fn insert_dvo_batch(
    conn: &libsql::Connection,
    code: &str,
    side: &str,
    status: &str,
) -> i64 {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO dvo_batch (code, warehouse_id, start_month, year, side, status, opened_at, created_at, updated_at) VALUES (?, 3, 11, 2025, ?, ?, ?, ?, ?)",
        params![code, side, status, now.clone(), now.clone(), now],
    )
    .await
    .unwrap();
    let mut rows = conn
        .query("SELECT id FROM dvo_batch WHERE code = ?", params![code])
        .await
        .unwrap();
    let r = rows.next().await.unwrap().unwrap();
    r.get(0).unwrap()
}

async fn insert_receipt(
    conn: &libsql::Connection,
    batch_id: i64,
    recv_date: &str,
    gothong: Option<&str>,
    dvo_kg: f64,
    cebu_kg: f64,
) {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO dvo_receipt (dvo_batch_id, recv_date, gothong_slip, sack_count, dvo_declared_weight_kg, cebu_declared_weight_kg, at_cebu_cy, created_at, updated_at) VALUES (?, ?, ?, NULL, ?, ?, 0, ?, ?)",
        params![batch_id, recv_date, gothong, dvo_kg, cebu_kg, now.clone(), now],
    )
    .await
    .unwrap();
}

async fn insert_dvo_outflow_event(
    conn: &libsql::Connection,
    batch_id: i64,
    recv_date: &str,
    weight: f64,
    disposition: &str,
) {
    let mut input = input(
        recv_date,
        "3X50",
        "DVO",
        Some("WHSE 3"),
        None,
        weight,
        None,
        disposition,
    );
    input.dvo_batch_id = Some(batch_id);
    insert_production_event_with_mode(conn, &input, InsertMode::Strict)
        .await
        .unwrap();
}

#[tokio::test]
async fn dvo_ledger_basic_running_kg_balance() {
    let (_tmp, _db, conn) = fresh_db().await;
    let bid = insert_dvo_batch(&conn, "NOVEMBER2025RIGHT", "RIGHT", "open").await;

    // Two receipts, one outflow.
    insert_receipt(&conn, bid, "2025-11-16", Some("G1"), 1900.0, 1820.0).await;
    insert_receipt(&conn, bid, "2025-11-18", Some("G2"), 1800.0, 1750.0).await;
    insert_dvo_outflow_event(&conn, bid, "2025-11-20", 800.0, "C1").await;

    let ledger = dvo_batch_ledger(&conn, bid).await.unwrap();

    assert_eq!(ledger.batch.code, "NOVEMBER2025RIGHT");
    assert_eq!(ledger.batch.status, DvoBatchStatus::Open);
    assert_eq!(ledger.receipts.len(), 2);
    assert_eq!(ledger.outflows.len(), 1);
    assert_eq!(ledger.interleaved.len(), 3);

    let bals: Vec<f64> = ledger
        .interleaved
        .iter()
        .map(|e| match e {
            DvoLedgerEvent::Receipt { run_bal_kg, .. } => *run_bal_kg,
            DvoLedgerEvent::Outflow { run_bal_kg, .. } => *run_bal_kg,
        })
        .collect();
    assert!((bals[0] - 1820.0).abs() < 1e-6); // first receipt
    assert!((bals[1] - 3570.0).abs() < 1e-6); // both receipts
    assert!((bals[2] - 2770.0).abs() < 1e-6); // − 800 outflow
}

#[tokio::test]
async fn dvo_ledger_loss_metrics() {
    let (_tmp, _db, conn) = fresh_db().await;
    let bid = insert_dvo_batch(&conn, "NOVEMBER2025RIGHT", "RIGHT", "open").await;

    // sum_dvo_declared = 4000; sum_cebu_declared = 3700; sum_partner_takes = 3300
    insert_receipt(&conn, bid, "2025-11-16", Some("G1"), 2000.0, 1850.0).await;
    insert_receipt(&conn, bid, "2025-11-18", Some("G2"), 2000.0, 1850.0).await;
    insert_dvo_outflow_event(&conn, bid, "2025-11-20", 1500.0, "C1").await;
    insert_dvo_outflow_event(&conn, bid, "2025-11-22", 1800.0, "RK1").await;

    let ledger = dvo_batch_ledger(&conn, bid).await.unwrap();

    // transit_loss = (4000 − 3700) / 4000 = 0.075
    assert!((ledger.transit_loss.value - 0.075).abs() < 1e-6);
    assert!((ledger.transit_loss.numerator_kg - 300.0).abs() < 1e-6);
    assert!((ledger.transit_loss.denominator_kg - 4000.0).abs() < 1e-6);
    assert!(!ledger.transit_loss.frozen);

    // yield_loss = (3700 − 3300) / 3700 ≈ 0.108108…
    assert!((ledger.yield_loss.value - (400.0 / 3700.0)).abs() < 1e-9);
    assert!((ledger.yield_loss.numerator_kg - 400.0).abs() < 1e-6);
    assert!((ledger.yield_loss.denominator_kg - 3700.0).abs() < 1e-6);
}

#[tokio::test]
async fn dvo_ledger_marks_frozen_when_batch_closed() {
    let (_tmp, _db, conn) = fresh_db().await;
    let bid = insert_dvo_batch(&conn, "OCTOBER2025LEFT", "LEFT", "closed").await;
    insert_receipt(&conn, bid, "2025-10-10", None, 1000.0, 950.0).await;

    let ledger = dvo_batch_ledger(&conn, bid).await.unwrap();
    assert_eq!(ledger.batch.status, DvoBatchStatus::Closed);
    assert!(ledger.transit_loss.frozen);
    assert!(ledger.yield_loss.frozen);
}
