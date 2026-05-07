//! Warehouse ledger functions.
//!
//! Two flavours, per docs/schema-extraction.md §6.7:
//! - `warehouse_ledger_flec` for WHSE 1/2/5/7 (flec count, per `(grade, side)`)
//! - `dvo_batch_ledger` for WHSE 3 (kg, scoped to one `dvo_batch`)
//!
//! Both return inputs alongside outputs (PROJECT_BRAIN.md §4.7 — always show
//! the solution). Step 5 implements the bodies; this file only locks in the
//! shapes so Step 3 can write commands against them and the type bridge can
//! generate TS interfaces.

use crate::canonicalize::{Disposition, Grade, Side, SourceCode, Warehouse};
use crate::error::Result;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    pub run_bal_flec: i64,
    pub run_bal_components: RunBalComponents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlecLedger {
    pub warehouse: Warehouse,
    pub start_date: NaiveDate,
    pub opening_balances: Vec<OpeningBalance>,
    pub rows: Vec<FlecLedgerRow>,
    /// Keyed by (grade, side) — the same shape the workbook's `STARTING` block
    /// uses. `Vec` not `HashMap` for serialization stability.
    pub current_balances: HashMap<String, i64>,
}

/// WHSE 1, 2, 5, 7 — per-(grade, side) running balance in flec count units.
///
/// Body lands in Step 5. The signature matches schema-extraction §6.7.
#[allow(unused_variables)]
pub async fn warehouse_ledger_flec(
    conn: &libsql::Connection,
    warehouse: Warehouse,
    start_date: NaiveDate,
) -> Result<FlecLedger> {
    unimplemented!(
        "warehouse_ledger_flec lands in Step 5 — see PROJECT_BRAIN.md §9 + \
         docs/schema-extraction.md §6.7"
    )
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
    pub opened_at: chrono::DateTime<chrono::Utc>,
    pub closed_at: Option<chrono::DateTime<chrono::Utc>>,
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
///
/// Body lands in Step 5. Schema-extraction §6.7.
#[allow(unused_variables)]
pub async fn dvo_batch_ledger(
    conn: &libsql::Connection,
    dvo_batch_id: i64,
) -> Result<DvoBatchLedger> {
    unimplemented!(
        "dvo_batch_ledger lands in Step 5 — see PROJECT_BRAIN.md §9 + \
         docs/schema-extraction.md §6.7"
    )
}
