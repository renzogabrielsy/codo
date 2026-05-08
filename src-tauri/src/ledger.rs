//! Warehouse ledger functions.
//!
//! Two flavours, per docs/schema-extraction.md §6.7:
//! - `warehouse_ledger_flec` for WHSE 1/2/5/7 (flec count, per `(grade, side)`)
//! - `dvo_batch_ledger` for WHSE 3 (kg, scoped to one `dvo_batch`)
//!
//! Both return inputs alongside outputs (PROJECT_BRAIN.md §4.7 — always show
//! the solution).

use crate::canonicalize::{Disposition, Grade, Side, SourceCode, Warehouse};
use crate::direction::{direction, Direction};
use crate::error::{CodoError, Result};
use chrono::NaiveDate;
use libsql::params;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// FLEC ledger — WHSE 1/2/5/7
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpeningBalance {
    pub grade: Grade,
    pub side: Side,
    pub flec_count: i64,
    pub period_start_date: NaiveDate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunBalComponents {
    pub opening: i64,
    pub flec_in_to_date: i64,
    pub flec_out_to_date: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlecLedgerRow {
    pub event_id: i64,
    pub unique_tag: String,
    pub recv_date: NaiveDate,
    pub prod_date: Option<NaiveDate>,
    pub source: SourceCode,
    pub grade: Grade,
    pub side: Option<Side>,
    pub disposition: Disposition,
    pub kg_in: Option<f64>,
    pub kg_out: Option<f64>,
    pub flec_in: Option<i64>,
    pub flec_out: Option<i64>,
    /// Running balance for this row's (grade, side). When `side` is None
    /// (legacy data with no side recorded) this stays 0 — the row is
    /// surfaced for visibility but doesn't move any (grade, side) balance.
    pub run_bal_flec: i64,
    pub run_bal_components: RunBalComponents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentBalance {
    pub grade: Grade,
    pub side: Side,
    pub flec_count: i64,
    pub components: RunBalComponents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlecLedger {
    pub warehouse: Warehouse,
    pub start_date: NaiveDate,
    pub opening_balances: Vec<OpeningBalance>,
    pub rows: Vec<FlecLedgerRow>,
    pub current_balances: Vec<CurrentBalance>,
    /// Rows that touch the warehouse but don't have a `whse_side` recorded.
    /// Surfaced as a count so the UI can flag "N events excluded from
    /// running balance — review legacy data". Each is a row in `rows` with
    /// `run_bal_flec = 0`.
    pub unsided_event_count: i64,
}

/// WHSE 1, 2, 5, 7 — per-(grade, side) running balance in flec count units.
pub async fn warehouse_ledger_flec(
    conn: &libsql::Connection,
    warehouse: Warehouse,
    start_date: NaiveDate,
) -> Result<FlecLedger> {
    if warehouse == Warehouse::W3 {
        return Err(CodoError::invalid(
            "WHSE 3 uses dvo_batch_ledger, not warehouse_ledger_flec",
        ));
    }
    let warehouse_id = warehouse_id_for(warehouse);
    let start_iso = start_date.format("%Y-%m-%d").to_string();

    // 1. Opening balances: most-recent row dated ≤ start_date per (grade, side).
    let opening_balances = fetch_opening_balances(conn, warehouse_id, &start_iso).await?;

    // 2. Walk every event affecting this warehouse from start_date forward,
    //    in chronological order (recv_date ASC, then id ASC for stable
    //    ordering of same-day rows).
    let raw = fetch_warehouse_events(conn, warehouse_id, &start_iso).await?;

    // Per-(grade, side) cumulative trackers.
    let mut state: std::collections::HashMap<(Grade, Side), RunBalComponents> =
        std::collections::HashMap::new();
    for ob in &opening_balances {
        state.insert(
            (ob.grade, ob.side),
            RunBalComponents {
                opening: ob.flec_count,
                flec_in_to_date: 0,
                flec_out_to_date: 0,
            },
        );
    }

    let mut rows: Vec<FlecLedgerRow> = Vec::with_capacity(raw.len());
    let mut unsided_event_count: i64 = 0;

    for ev in raw {
        let dir = direction(ev.disposition, ev.source.kind(), true);
        // direction() should always return Some for warehouse-attached events;
        // defensive None handling just shows the row with run_bal=0.
        let (flec_in, flec_out, kg_in, kg_out) = match dir {
            Some(Direction::In) => (
                ev.flec_count.filter(|n| *n > 0),
                None,
                Some(ev.weight_kg),
                None,
            ),
            Some(Direction::Out) => (
                None,
                ev.flec_count.filter(|n| *n > 0),
                None,
                Some(ev.weight_kg),
            ),
            None => (None, None, None, None),
        };

        let (run_bal, components) = match ev.side {
            None => {
                unsided_event_count += 1;
                (
                    0_i64,
                    RunBalComponents {
                        opening: 0,
                        flec_in_to_date: 0,
                        flec_out_to_date: 0,
                    },
                )
            }
            Some(side) => {
                let entry = state.entry((ev.grade, side)).or_insert(RunBalComponents {
                    opening: 0,
                    flec_in_to_date: 0,
                    flec_out_to_date: 0,
                });
                entry.flec_in_to_date += flec_in.unwrap_or(0);
                entry.flec_out_to_date += flec_out.unwrap_or(0);
                let bal = entry.opening + entry.flec_in_to_date - entry.flec_out_to_date;
                (bal, entry.clone())
            }
        };

        rows.push(FlecLedgerRow {
            event_id: ev.event_id,
            unique_tag: ev.unique_tag,
            recv_date: ev.recv_date,
            prod_date: ev.prod_date,
            source: ev.source,
            grade: ev.grade,
            side: ev.side,
            disposition: ev.disposition,
            kg_in,
            kg_out,
            flec_in,
            flec_out,
            run_bal_flec: run_bal,
            run_bal_components: components,
        });
    }

    // 3. Snapshot current balances per (grade, side).
    let mut current_balances: Vec<CurrentBalance> = state
        .into_iter()
        .map(|((grade, side), c)| CurrentBalance {
            grade,
            side,
            flec_count: c.opening + c.flec_in_to_date - c.flec_out_to_date,
            components: c,
        })
        .collect();
    current_balances.sort_by(|a, b| {
        a.grade
            .code()
            .cmp(b.grade.code())
            .then_with(|| a.side.code().cmp(b.side.code()))
    });

    Ok(FlecLedger {
        warehouse,
        start_date,
        opening_balances,
        rows,
        current_balances,
        unsided_event_count,
    })
}

// ---------------------------------------------------------------------------
// FLEC ledger — internal helpers
// ---------------------------------------------------------------------------

struct RawEvent {
    event_id: i64,
    unique_tag: String,
    recv_date: NaiveDate,
    prod_date: Option<NaiveDate>,
    source: SourceCode,
    grade: Grade,
    side: Option<Side>,
    disposition: Disposition,
    flec_count: Option<i64>,
    weight_kg: f64,
}

async fn fetch_opening_balances(
    conn: &libsql::Connection,
    warehouse_id: i64,
    start_iso: &str,
) -> Result<Vec<OpeningBalance>> {
    let mut rows = conn
        .query(
            "SELECT gr.code, ob.side, ob.opening_flec_count, ob.period_start_date
             FROM warehouse_opening_balance ob
             JOIN grade gr ON gr.id = ob.grade_id
             WHERE ob.warehouse_id = ? AND ob.period_start_date <= ?
             ORDER BY ob.grade_id, ob.side, ob.period_start_date DESC",
            params![warehouse_id, start_iso],
        )
        .await?;

    // Take first row per (grade, side) — most recent opening on/before start.
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let mut out = Vec::new();
    while let Some(r) = rows.next().await? {
        let grade_code: String = r.get(0)?;
        let side_code: String = r.get(1)?;
        let key = (grade_code.clone(), side_code.clone());
        if !seen.insert(key) {
            continue;
        }
        let count: i64 = r.get(2)?;
        let date_iso: String = r.get(3)?;
        let grade = parse_grade(&grade_code)?;
        let side = parse_side(&side_code)?;
        let date = NaiveDate::parse_from_str(&date_iso, "%Y-%m-%d")
            .map_err(|e| CodoError::internal(format!("opening_balance date parse: {e}")))?;
        out.push(OpeningBalance {
            grade,
            side,
            flec_count: count,
            period_start_date: date,
        });
    }
    Ok(out)
}

async fn fetch_warehouse_events(
    conn: &libsql::Connection,
    warehouse_id: i64,
    start_iso: &str,
) -> Result<Vec<RawEvent>> {
    let mut rows = conn
        .query(
            "SELECT pe.id, pe.unique_tag, pe.recv_date, pe.prod_date,
                    sl.code AS src_code,
                    gr.code AS grade_code,
                    pe.whse_side, pe.disposition_kind,
                    peq.code AS partner_eq_code,
                    pe.flec_count, pe.weight_kg
             FROM production_event pe
             JOIN source_location sl ON sl.id = pe.source_location_id
             JOIN grade gr           ON gr.id = pe.grade_id
             LEFT JOIN partner_equipment peq ON peq.id = pe.partner_equipment_id
             WHERE pe.warehouse_id = ? AND pe.recv_date >= ?
             ORDER BY pe.recv_date, pe.id",
            params![warehouse_id, start_iso],
        )
        .await?;
    let mut out = Vec::new();
    while let Some(r) = rows.next().await? {
        let event_id: i64 = r.get(0)?;
        let unique_tag: String = r.get(1)?;
        let recv_iso: String = r.get(2)?;
        let prod_iso: Option<String> = r.get(3).ok();
        let src_code: String = r.get(4)?;
        let grade_code: String = r.get(5)?;
        let whse_side: Option<String> = r.get(6).ok();
        let disposition_kind: String = r.get(7)?;
        let partner_eq: Option<String> = r.get(8).ok();
        let flec_count: Option<i64> = r.get(9).ok();
        let weight_kg: f64 = r.get(10)?;

        let recv_date = NaiveDate::parse_from_str(&recv_iso, "%Y-%m-%d")
            .map_err(|e| CodoError::internal(format!("recv_date parse: {e}")))?;
        let prod_date = prod_iso
            .as_ref()
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());

        let source = parse_source_code(&src_code)?;
        let grade = parse_grade(&grade_code)?;
        let side = match whse_side.as_deref() {
            Some("LS") => Some(Side::Ls),
            Some("RS") => Some(Side::Rs),
            _ => None,
        };
        let disposition = build_disposition(&disposition_kind, partner_eq.as_deref())?;

        out.push(RawEvent {
            event_id,
            unique_tag,
            recv_date,
            prod_date,
            source,
            grade,
            side,
            disposition,
            flec_count,
            weight_kg,
        });
    }
    Ok(out)
}

fn parse_grade(code: &str) -> Result<Grade> {
    Ok(match code {
        "3X50" => Grade::G3x50,
        "2X6" => Grade::G2x6,
        "3.5" => Grade::G3p5,
        "4X8" => Grade::G4x8,
        other => {
            return Err(CodoError::internal(format!(
                "unknown grade code in DB: {other}"
            )))
        }
    })
}
fn parse_side(code: &str) -> Result<Side> {
    Ok(match code {
        "LS" => Side::Ls,
        "RS" => Side::Rs,
        other => {
            return Err(CodoError::internal(format!(
                "unknown side code in DB: {other}"
            )))
        }
    })
}
fn parse_source_code(code: &str) -> Result<SourceCode> {
    Ok(match code {
        "TNK 1" => SourceCode::Tnk1,
        "TNK 2" => SourceCode::Tnk2,
        "TNK 3" => SourceCode::Tnk3,
        "TNK 4" => SourceCode::Tnk4,
        "W7" => SourceCode::W7,
        "W6" => SourceCode::W6,
        "FLEC" => SourceCode::Flec,
        "DVO" => SourceCode::Dvo,
        other => {
            return Err(CodoError::internal(format!(
                "unknown source_location code in DB: {other}"
            )))
        }
    })
}
fn build_disposition(kind: &str, partner_eq: Option<&str>) -> Result<Disposition> {
    Ok(match kind {
        "flec_bagging" => Disposition::FlecBagging,
        "partner_crusher" => match partner_eq.unwrap_or("") {
            "C1" => Disposition::PartnerCrusher(1),
            "C2" => Disposition::PartnerCrusher(2),
            "C3" => Disposition::PartnerCrusher(3),
            "C4" => Disposition::PartnerCrusher(4),
            other => {
                return Err(CodoError::internal(format!(
                    "unknown partner_crusher equipment: {other}"
                )))
            }
        },
        "partner_kiln" => match partner_eq.unwrap_or("") {
            "RK1" => Disposition::PartnerKiln(1),
            "RK2" => Disposition::PartnerKiln(2),
            "RK3" => Disposition::PartnerKiln(3),
            "RK4" => Disposition::PartnerKiln(4),
            other => {
                return Err(CodoError::internal(format!(
                    "unknown partner_kiln equipment: {other}"
                )))
            }
        },
        other => {
            return Err(CodoError::internal(format!(
                "unknown disposition_kind: {other}"
            )))
        }
    })
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

// ---------------------------------------------------------------------------
// DVO batch ledger — WHSE 3, scoped per `dvo_batch`
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DvoBatch {
    pub id: i64,
    pub code: String,
    pub year: i32,
    pub start_month: u8,
    pub side: DvoBatchSide,
    pub status: DvoBatchStatus,
    pub opened_at: String,
    pub closed_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DvoBatchSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DvoBatchStatus {
    Open,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DvoReceiptRow {
    pub id: i64,
    pub recv_date: NaiveDate,
    pub gothong_slip: Option<String>,
    pub sack_count: Option<i64>,
    pub dvo_declared_weight_kg: f64,
    pub cebu_declared_weight_kg: f64,
    pub at_cebu_cy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DvoOutflowRow {
    pub event_id: i64,
    pub unique_tag: String,
    pub recv_date: NaiveDate,
    pub prod_date: Option<NaiveDate>,
    pub disposition: Disposition,
    pub weight_kg: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum DvoLedgerEvent {
    Receipt {
        receipt: DvoReceiptRow,
        run_bal_kg: f64,
    },
    Outflow {
        outflow: DvoOutflowRow,
        run_bal_kg: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LossMetric {
    pub value: f64,
    pub numerator_kg: f64,
    pub denominator_kg: f64,
    pub frozen: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DvoBatchLedger {
    pub batch: DvoBatch,
    pub receipts: Vec<DvoReceiptRow>,
    pub outflows: Vec<DvoOutflowRow>,
    pub interleaved: Vec<DvoLedgerEvent>,
    pub transit_loss: LossMetric,
    pub yield_loss: LossMetric,
}

/// WHSE 3 — kg-balance ledger scoped to a single `dvo_batch`.
pub async fn dvo_batch_ledger(
    conn: &libsql::Connection,
    dvo_batch_id: i64,
) -> Result<DvoBatchLedger> {
    let batch = fetch_dvo_batch(conn, dvo_batch_id).await?;
    let receipts = fetch_dvo_receipts(conn, dvo_batch_id).await?;
    let outflows = fetch_dvo_outflows(conn, dvo_batch_id).await?;
    let interleaved = interleave_kg_running_balance(&receipts, &outflows);
    let (transit_loss, yield_loss) = compute_loss_metrics(&batch, &receipts, &outflows);
    Ok(DvoBatchLedger {
        batch,
        receipts,
        outflows,
        interleaved,
        transit_loss,
        yield_loss,
    })
}

async fn fetch_dvo_batch(conn: &libsql::Connection, id: i64) -> Result<DvoBatch> {
    let mut rows = conn
        .query(
            "SELECT id, code, year, start_month, side, status, opened_at, closed_at
             FROM dvo_batch WHERE id = ?",
            params![id],
        )
        .await?;
    let r = rows
        .next()
        .await?
        .ok_or_else(|| CodoError::invalid(format!("dvo_batch id={id} not found")))?;
    let id: i64 = r.get(0)?;
    let code: String = r.get(1)?;
    let year: i64 = r.get(2)?;
    let start_month: i64 = r.get(3)?;
    let side_str: String = r.get(4)?;
    let status_str: String = r.get(5)?;
    let opened_at: String = r.get(6)?;
    let closed_at: Option<String> = r.get(7).ok();
    Ok(DvoBatch {
        id,
        code,
        year: year as i32,
        start_month: start_month as u8,
        side: match side_str.as_str() {
            "LEFT" => DvoBatchSide::Left,
            "RIGHT" => DvoBatchSide::Right,
            other => {
                return Err(CodoError::internal(format!(
                    "unknown dvo_batch.side: {other}"
                )))
            }
        },
        status: match status_str.as_str() {
            "open" => DvoBatchStatus::Open,
            "closed" => DvoBatchStatus::Closed,
            other => {
                return Err(CodoError::internal(format!(
                    "unknown dvo_batch.status: {other}"
                )))
            }
        },
        opened_at,
        closed_at,
    })
}

async fn fetch_dvo_receipts(conn: &libsql::Connection, batch_id: i64) -> Result<Vec<DvoReceiptRow>> {
    let mut rows = conn
        .query(
            "SELECT id, recv_date, gothong_slip, sack_count,
                    dvo_declared_weight_kg, cebu_declared_weight_kg, at_cebu_cy
             FROM dvo_receipt WHERE dvo_batch_id = ?
             ORDER BY recv_date, id",
            params![batch_id],
        )
        .await?;
    let mut out = Vec::new();
    while let Some(r) = rows.next().await? {
        let id: i64 = r.get(0)?;
        let recv_iso: String = r.get(1)?;
        let gothong: Option<String> = r.get(2).ok();
        let sack_count: Option<i64> = r.get(3).ok();
        let dvo_declared: f64 = r.get(4)?;
        let cebu_declared: f64 = r.get(5)?;
        let at_cy: i64 = r.get(6)?;
        out.push(DvoReceiptRow {
            id,
            recv_date: NaiveDate::parse_from_str(&recv_iso, "%Y-%m-%d")
                .map_err(|e| CodoError::internal(format!("recv_date parse: {e}")))?,
            gothong_slip: gothong,
            sack_count,
            dvo_declared_weight_kg: dvo_declared,
            cebu_declared_weight_kg: cebu_declared,
            at_cebu_cy: at_cy != 0,
        });
    }
    Ok(out)
}

async fn fetch_dvo_outflows(conn: &libsql::Connection, batch_id: i64) -> Result<Vec<DvoOutflowRow>> {
    let mut rows = conn
        .query(
            "SELECT pe.id, pe.unique_tag, pe.recv_date, pe.prod_date,
                    pe.disposition_kind, peq.code AS partner_eq_code, pe.weight_kg
             FROM production_event pe
             LEFT JOIN partner_equipment peq ON peq.id = pe.partner_equipment_id
             WHERE pe.dvo_batch_id = ?
             ORDER BY pe.recv_date, pe.id",
            params![batch_id],
        )
        .await?;
    let mut out = Vec::new();
    while let Some(r) = rows.next().await? {
        let event_id: i64 = r.get(0)?;
        let unique_tag: String = r.get(1)?;
        let recv_iso: String = r.get(2)?;
        let prod_iso: Option<String> = r.get(3).ok();
        let disposition_kind: String = r.get(4)?;
        let partner_eq: Option<String> = r.get(5).ok();
        let weight_kg: f64 = r.get(6)?;
        out.push(DvoOutflowRow {
            event_id,
            unique_tag,
            recv_date: NaiveDate::parse_from_str(&recv_iso, "%Y-%m-%d")
                .map_err(|e| CodoError::internal(format!("recv_date parse: {e}")))?,
            prod_date: prod_iso
                .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
            disposition: build_disposition(&disposition_kind, partner_eq.as_deref())?,
            weight_kg,
        });
    }
    Ok(out)
}

fn interleave_kg_running_balance(
    receipts: &[DvoReceiptRow],
    outflows: &[DvoOutflowRow],
) -> Vec<DvoLedgerEvent> {
    // Combine + sort by date, with receipts BEFORE outflows on the same day
    // (cebu scale before partner takeback by convention).
    let mut combined: Vec<(NaiveDate, u8, usize)> = Vec::new();
    for (i, r) in receipts.iter().enumerate() {
        combined.push((r.recv_date, 0, i));
    }
    for (i, o) in outflows.iter().enumerate() {
        combined.push((o.recv_date, 1, i));
    }
    combined.sort();

    let mut bal: f64 = 0.0;
    let mut out = Vec::with_capacity(combined.len());
    for (_, kind, idx) in combined {
        if kind == 0 {
            let r = &receipts[idx];
            bal += r.cebu_declared_weight_kg;
            out.push(DvoLedgerEvent::Receipt {
                receipt: r.clone(),
                run_bal_kg: bal,
            });
        } else {
            let o = &outflows[idx];
            bal -= o.weight_kg;
            out.push(DvoLedgerEvent::Outflow {
                outflow: o.clone(),
                run_bal_kg: bal,
            });
        }
    }
    out
}

fn compute_loss_metrics(
    batch: &DvoBatch,
    receipts: &[DvoReceiptRow],
    outflows: &[DvoOutflowRow],
) -> (LossMetric, LossMetric) {
    let frozen = batch.status == DvoBatchStatus::Closed;
    let sum_dvo: f64 = receipts.iter().map(|r| r.dvo_declared_weight_kg).sum();
    let sum_cebu: f64 = receipts.iter().map(|r| r.cebu_declared_weight_kg).sum();
    let sum_partner: f64 = outflows.iter().map(|o| o.weight_kg).sum();

    // transit_loss = (sum_dvo − sum_cebu) / sum_dvo
    let transit_loss = LossMetric {
        value: if sum_dvo > 0.0 {
            (sum_dvo - sum_cebu) / sum_dvo
        } else {
            0.0
        },
        numerator_kg: sum_dvo - sum_cebu,
        denominator_kg: sum_dvo,
        frozen,
    };
    // yield_loss = (sum_cebu − sum_partner) / sum_cebu
    let yield_loss = LossMetric {
        value: if sum_cebu > 0.0 {
            (sum_cebu - sum_partner) / sum_cebu
        } else {
            0.0
        },
        numerator_kg: sum_cebu - sum_partner,
        denominator_kg: sum_cebu,
        frozen,
    };
    (transit_loss, yield_loss)
}
