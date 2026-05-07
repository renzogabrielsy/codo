//! Validity matrix lock-in tests.
//!
//! Walks every cell of docs/schema-extraction.md §7.1. Every VALID combination
//! must:
//!   1. pass `validate_production_event` and
//!   2. insert into the live schema via libSQL (i.e. the DB-level CHECK
//!      constraints in migrations/v1.sql don't reject it).
//! Every FORBIDDEN combination must fail `validate_production_event` with the
//! documented reason, before ever reaching the DB.
//!
//! This is the test scaffold Step 3 (the "Log a production event" form) will
//! build against — see PROJECT_BRAIN.md §9.

use codo_lib::canonicalize::{Disposition, Plant, SourceCode, Warehouse};
use codo_lib::db;
use codo_lib::validation::{validate_production_event, EventShape, ValidationError};
use libsql::params;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn shape(
    disposition: Disposition,
    source: SourceCode,
    warehouse: Option<Warehouse>,
    plant: Option<Plant>,
) -> EventShape {
    EventShape {
        disposition,
        source,
        warehouse,
        plant,
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

fn plant_id_for(p: Plant) -> i64 {
    match p {
        Plant::W6 => 1,
        Plant::W7 => 2,
        Plant::W6W7 => 3,
        Plant::Dvo => 4,
    }
}

fn source_location_id_for(s: SourceCode) -> i64 {
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

fn partner_equipment_id_for(code: &str) -> Option<i64> {
    Some(match code {
        "C1" => 1,
        "C2" => 2,
        "C3" => 3,
        "C4" => 4,
        "RK1" => 11,
        "RK2" => 12,
        "RK3" => 13,
        "RK4" => 14,
        _ => return None,
    })
}

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

async fn live_insert(
    conn: &libsql::Connection,
    e: EventShape,
    label: &str,
    n: u64,
) -> Result<(), libsql::Error> {
    let tag = format!("test-{label}-{n}");
    let now = chrono::Utc::now().to_rfc3339();
    let warehouse_id = e.warehouse.map(warehouse_id_for);
    let plant_id = e.plant.map(plant_id_for);
    let partner_id = e
        .disposition
        .equipment_code()
        .and_then(|c| partner_equipment_id_for(&c));

    conn.execute(
        "INSERT INTO production_event (
            recv_date, prod_date, batch,
            shift_id, grade_id, plant_id, warehouse_id, source_location_id,
            weight_kg, disposition_kind, partner_equipment_id,
            flec_count, whse_side, flec_stat, dvo_batch_id,
            unique_tag, notes, dirty, created_at, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
        params![
            "2026-05-01",
            Option::<String>::None,
            "MAY",
            1_i64,
            1_i64,
            plant_id,
            warehouse_id,
            source_location_id_for(e.source),
            1234.5_f64,
            e.disposition.kind_code(),
            partner_id,
            Option::<i64>::None,
            Option::<String>::None,
            Option::<String>::None,
            Option::<i64>::None,
            tag,
            Option::<String>::None,
            now.clone(),
            now,
        ],
    )
    .await
    .map(|_| ())
}

// ---------------------------------------------------------------------------
// Pure validator coverage — every row of §7.1 by name.
// ---------------------------------------------------------------------------

#[test]
fn flec_bagging_tank_into_warehouse_1257_is_valid() {
    for w in [Warehouse::W1, Warehouse::W2, Warehouse::W5, Warehouse::W7] {
        for src in [
            SourceCode::Tnk1,
            SourceCode::Tnk2,
            SourceCode::Tnk3,
            SourceCode::Tnk4,
        ] {
            assert_eq!(
                validate_production_event(shape(
                    Disposition::FlecBagging,
                    src,
                    Some(w),
                    Some(Plant::W6),
                )),
                Ok(())
            );
        }
        // W7 source (the W7 plant tank) → plant=W7 forced.
        assert_eq!(
            validate_production_event(shape(
                Disposition::FlecBagging,
                SourceCode::W7,
                Some(w),
                Some(Plant::W7),
            )),
            Ok(())
        );
    }
}

#[test]
fn flec_bagging_tank_into_whse3_is_forbidden() {
    let err = validate_production_event(shape(
        Disposition::FlecBagging,
        SourceCode::Tnk1,
        Some(Warehouse::W3),
        Some(Plant::W6),
    ))
    .unwrap_err();
    assert!(matches!(err, ValidationError::Forbidden { .. }));
}

#[test]
fn flec_bagging_with_no_warehouse_is_forbidden() {
    for src in [SourceCode::Tnk1, SourceCode::W6, SourceCode::W7] {
        let plant = match src {
            SourceCode::W7 => Plant::W7,
            _ => Plant::W6,
        };
        let err = validate_production_event(shape(
            Disposition::FlecBagging,
            src,
            None,
            Some(plant),
        ))
        .unwrap_err();
        assert!(
            matches!(err, ValidationError::Forbidden { .. }),
            "expected Forbidden for src={src:?} no-warehouse, got {err:?}"
        );
    }
}

#[test]
fn flec_bagging_plant_direct_into_warehouse_1257_is_valid() {
    for w in [Warehouse::W1, Warehouse::W2, Warehouse::W5, Warehouse::W7] {
        assert_eq!(
            validate_production_event(shape(
                Disposition::FlecBagging,
                SourceCode::W6,
                Some(w),
                Some(Plant::W6),
            )),
            Ok(())
        );
    }
}

#[test]
fn flec_bagging_plant_direct_into_whse3_is_forbidden() {
    let err = validate_production_event(shape(
        Disposition::FlecBagging,
        SourceCode::W6,
        Some(Warehouse::W3),
        Some(Plant::W6),
    ))
    .unwrap_err();
    assert!(matches!(err, ValidationError::Forbidden { .. }));
}

#[test]
fn flec_bagging_from_warehouse_flec_is_forbidden() {
    for w in [Warehouse::W1, Warehouse::W7, Warehouse::W3] {
        let err = validate_production_event(shape(
            Disposition::FlecBagging,
            SourceCode::Flec,
            Some(w),
            None,
        ))
        .unwrap_err();
        assert!(
            matches!(err, ValidationError::Forbidden { .. }),
            "expected Forbidden for FLEC source × {w:?}"
        );
    }
}

#[test]
fn flec_bagging_from_dvo_is_forbidden() {
    let err = validate_production_event(shape(
        Disposition::FlecBagging,
        SourceCode::Dvo,
        Some(Warehouse::W3),
        Some(Plant::Dvo),
    ))
    .unwrap_err();
    assert!(matches!(err, ValidationError::Forbidden { .. }));
}

#[test]
fn partner_pull_from_tank_no_warehouse_is_valid() {
    for disp in [
        Disposition::PartnerCrusher(1),
        Disposition::PartnerCrusher(4),
        Disposition::PartnerKiln(2),
    ] {
        for src in [
            SourceCode::Tnk1,
            SourceCode::Tnk2,
            SourceCode::Tnk3,
            SourceCode::Tnk4,
        ] {
            assert_eq!(
                validate_production_event(shape(disp, src, None, Some(Plant::W6))),
                Ok(())
            );
        }
        assert_eq!(
            validate_production_event(shape(
                disp,
                SourceCode::W7,
                None,
                Some(Plant::W7),
            )),
            Ok(())
        );
    }
}

#[test]
fn partner_pull_from_tank_with_warehouse_is_forbidden() {
    let err = validate_production_event(shape(
        Disposition::PartnerCrusher(1),
        SourceCode::Tnk1,
        Some(Warehouse::W7),
        Some(Plant::W6),
    ))
    .unwrap_err();
    assert!(matches!(err, ValidationError::Forbidden { .. }));
}

#[test]
fn partner_pull_from_plant_direct_no_warehouse_is_valid() {
    assert_eq!(
        validate_production_event(shape(
            Disposition::PartnerCrusher(2),
            SourceCode::W6,
            None,
            Some(Plant::W6),
        )),
        Ok(())
    );
}

#[test]
fn partner_pull_from_plant_direct_with_warehouse_is_forbidden() {
    let err = validate_production_event(shape(
        Disposition::PartnerCrusher(2),
        SourceCode::W6,
        Some(Warehouse::W7),
        Some(Plant::W6),
    ))
    .unwrap_err();
    assert!(matches!(err, ValidationError::Forbidden { .. }));
}

#[test]
fn partner_pull_from_warehouse_flec_into_1257_is_valid() {
    for w in [Warehouse::W1, Warehouse::W2, Warehouse::W5, Warehouse::W7] {
        assert_eq!(
            validate_production_event(shape(
                Disposition::PartnerKiln(3),
                SourceCode::Flec,
                Some(w),
                None,
            )),
            Ok(())
        );
    }
}

#[test]
fn partner_pull_from_warehouse_flec_into_whse3_is_forbidden() {
    let err = validate_production_event(shape(
        Disposition::PartnerKiln(3),
        SourceCode::Flec,
        Some(Warehouse::W3),
        None,
    ))
    .unwrap_err();
    assert!(matches!(err, ValidationError::Forbidden { .. }));
}

#[test]
fn partner_pull_from_warehouse_flec_no_warehouse_is_forbidden() {
    let err = validate_production_event(shape(
        Disposition::PartnerKiln(3),
        SourceCode::Flec,
        None,
        None,
    ))
    .unwrap_err();
    assert!(matches!(err, ValidationError::Forbidden { .. }));
}

#[test]
fn partner_pull_from_dvo_into_whse3_is_valid() {
    assert_eq!(
        validate_production_event(shape(
            Disposition::PartnerCrusher(1),
            SourceCode::Dvo,
            Some(Warehouse::W3),
            Some(Plant::Dvo),
        )),
        Ok(())
    );
}

#[test]
fn partner_pull_from_dvo_into_other_warehouses_is_forbidden() {
    for w in [Warehouse::W1, Warehouse::W2, Warehouse::W5, Warehouse::W7] {
        let err = validate_production_event(shape(
            Disposition::PartnerCrusher(1),
            SourceCode::Dvo,
            Some(w),
            Some(Plant::Dvo),
        ))
        .unwrap_err();
        assert!(matches!(err, ValidationError::Forbidden { .. }));
    }
}

#[test]
fn partner_pull_from_dvo_no_warehouse_is_forbidden() {
    let err = validate_production_event(shape(
        Disposition::PartnerCrusher(1),
        SourceCode::Dvo,
        None,
        Some(Plant::Dvo),
    ))
    .unwrap_err();
    assert!(matches!(err, ValidationError::Forbidden { .. }));
}

// ---------------------------------------------------------------------------
// SRC ↔ PLANT pairing (§7.2 rules 23–27).
// ---------------------------------------------------------------------------

#[test]
fn tnk_sources_force_plant_w6() {
    for src in [
        SourceCode::Tnk1,
        SourceCode::Tnk2,
        SourceCode::Tnk3,
        SourceCode::Tnk4,
    ] {
        let err = validate_production_event(shape(
            Disposition::FlecBagging,
            src,
            Some(Warehouse::W7),
            Some(Plant::W7),
        ))
        .unwrap_err();
        assert!(
            matches!(err, ValidationError::PlantMismatch { .. }),
            "expected PlantMismatch for tank source with PLANT=W7, got {err:?}"
        );
    }
}

#[test]
fn w7_source_forces_plant_w7() {
    let err = validate_production_event(shape(
        Disposition::FlecBagging,
        SourceCode::W7,
        Some(Warehouse::W7),
        Some(Plant::W6),
    ))
    .unwrap_err();
    assert!(matches!(err, ValidationError::PlantMismatch { .. }));
}

#[test]
fn dvo_source_forces_plant_dvo() {
    let err = validate_production_event(shape(
        Disposition::PartnerCrusher(1),
        SourceCode::Dvo,
        Some(Warehouse::W3),
        Some(Plant::W6),
    ))
    .unwrap_err();
    assert!(matches!(err, ValidationError::PlantMismatch { .. }));
}

#[test]
fn flec_source_allows_any_plant_or_none() {
    for plant in [None, Some(Plant::W6), Some(Plant::W7), Some(Plant::Dvo)] {
        assert_eq!(
            validate_production_event(shape(
                Disposition::PartnerCrusher(2),
                SourceCode::Flec,
                Some(Warehouse::W7),
                plant,
            )),
            Ok(())
        );
    }
}

// ---------------------------------------------------------------------------
// Live-DB end-to-end fixtures.
// ---------------------------------------------------------------------------

const VALID_FIXTURES: &[(&str, Disposition, SourceCode, Option<Warehouse>, Option<Plant>)] = &[
    (
        "ci_bag_tnk1_w7",
        Disposition::FlecBagging,
        SourceCode::Tnk1,
        Some(Warehouse::W7),
        Some(Plant::W6),
    ),
    (
        "ci_bag_w6_w1",
        Disposition::FlecBagging,
        SourceCode::W6,
        Some(Warehouse::W1),
        Some(Plant::W6),
    ),
    (
        "ci_bag_w7_w5",
        Disposition::FlecBagging,
        SourceCode::W7,
        Some(Warehouse::W5),
        Some(Plant::W7),
    ),
    (
        "partner_tank_takeback",
        Disposition::PartnerCrusher(1),
        SourceCode::Tnk2,
        None,
        Some(Plant::W6),
    ),
    (
        "partner_plant_direct_takeback",
        Disposition::PartnerKiln(3),
        SourceCode::W6,
        None,
        Some(Plant::W6),
    ),
    (
        "partner_warehouse_flec_w7",
        Disposition::PartnerKiln(4),
        SourceCode::Flec,
        Some(Warehouse::W7),
        None,
    ),
    (
        "partner_dvo_w3",
        Disposition::PartnerCrusher(2),
        SourceCode::Dvo,
        Some(Warehouse::W3),
        Some(Plant::Dvo),
    ),
];

const FORBIDDEN_FIXTURES: &[(&str, Disposition, SourceCode, Option<Warehouse>, Option<Plant>)] = &[
    (
        "ci_bag_tank_into_w3",
        Disposition::FlecBagging,
        SourceCode::Tnk1,
        Some(Warehouse::W3),
        Some(Plant::W6),
    ),
    (
        "ci_bag_tank_no_warehouse",
        Disposition::FlecBagging,
        SourceCode::Tnk1,
        None,
        Some(Plant::W6),
    ),
    (
        "ci_bag_from_flec",
        Disposition::FlecBagging,
        SourceCode::Flec,
        Some(Warehouse::W7),
        None,
    ),
    (
        "ci_bag_from_dvo",
        Disposition::FlecBagging,
        SourceCode::Dvo,
        Some(Warehouse::W3),
        Some(Plant::Dvo),
    ),
    (
        "partner_tank_with_warehouse",
        Disposition::PartnerCrusher(1),
        SourceCode::Tnk1,
        Some(Warehouse::W7),
        Some(Plant::W6),
    ),
    (
        "partner_plant_direct_with_warehouse",
        Disposition::PartnerKiln(2),
        SourceCode::W6,
        Some(Warehouse::W1),
        Some(Plant::W6),
    ),
    (
        "partner_flec_into_w3",
        Disposition::PartnerCrusher(1),
        SourceCode::Flec,
        Some(Warehouse::W3),
        None,
    ),
    (
        "partner_flec_no_warehouse",
        Disposition::PartnerCrusher(1),
        SourceCode::Flec,
        None,
        None,
    ),
    (
        "partner_dvo_no_warehouse",
        Disposition::PartnerCrusher(1),
        SourceCode::Dvo,
        None,
        Some(Plant::Dvo),
    ),
    (
        "partner_dvo_into_w7",
        Disposition::PartnerCrusher(1),
        SourceCode::Dvo,
        Some(Warehouse::W7),
        Some(Plant::Dvo),
    ),
    (
        "tank_with_wrong_plant",
        Disposition::FlecBagging,
        SourceCode::Tnk1,
        Some(Warehouse::W7),
        Some(Plant::W7),
    ),
];

#[tokio::test]
async fn every_valid_fixture_inserts_cleanly() {
    let (_tmp, _db, conn) = fresh_db().await;

    for (i, (label, d, s, w, p)) in VALID_FIXTURES.iter().enumerate() {
        let e = shape(*d, *s, *w, *p);
        validate_production_event(e)
            .unwrap_or_else(|err| panic!("VALID fixture {label} failed validation: {err:?}"));
        live_insert(&conn, e, label, i as u64 + 1)
            .await
            .unwrap_or_else(|err| panic!("VALID fixture {label} failed insert: {err:?}"));
    }
}

#[tokio::test]
async fn every_forbidden_fixture_is_rejected_before_insert() {
    let (_tmp, _db, _conn) = fresh_db().await;

    for (label, d, s, w, p) in FORBIDDEN_FIXTURES {
        let e = shape(*d, *s, *w, *p);
        let result = validate_production_event(e);
        assert!(
            result.is_err(),
            "FORBIDDEN fixture {label} unexpectedly passed validation \
             — schema-extraction.md §7.1 says it must be rejected"
        );
    }
}

#[tokio::test]
async fn db_has_seeded_lookup_tables() {
    let (_tmp, _db, conn) = fresh_db().await;

    let mut rows = conn
        .query("SELECT COUNT(*) FROM warehouse", params![])
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    let n: i64 = row.get(0).unwrap();
    assert_eq!(n, 5, "5 warehouses seeded (WHSE 1/2/3/5/7)");

    let mut rows = conn
        .query("SELECT COUNT(*) FROM partner_equipment", params![])
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    let n: i64 = row.get(0).unwrap();
    assert_eq!(n, 8, "4 crushers + 4 kilns seeded");

    let mut rows = conn
        .query("SELECT COUNT(*) FROM source_location", params![])
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    let n: i64 = row.get(0).unwrap();
    assert_eq!(n, 8, "TNK1..4, W7, W6, FLEC, DVO seeded");
}
