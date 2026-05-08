// Hand-written TS interfaces for codo's Tauri command surface.
// (tauri-specta type bridge punted per PROJECT_BRAIN.md §5 #9 / Plan B —
// hand-write while there are <20 commands; revisit when the surface grows.)
//
// These MUST match the Rust structs in src-tauri/src/commands.rs. If you
// edit one side, edit the other too. cargo test catches mismatches at the
// integration level via tests/validity_matrix.rs.

export type LookupRow = {
  id: number;
  code: string;
  display_name: string;
};

export type GradeRow = LookupRow & {
  expected_kg_per_bag_min: number | null;
  expected_kg_per_bag_max: number | null;
};

export type WarehouseRow = {
  id: number;
  code: string;
  default_unit: string;
};

export type SourceLocationRow = {
  id: number;
  code: string;
  display_name: string;
  /** 'tank' | 'plant_direct' | 'warehouse_flec' | 'dvo_container' */
  kind: string;
  plant_id: number | null;
};

export type PartnerEquipmentRow = {
  id: number;
  code: string;
  display_name: string;
  /** 'crusher' | 'kiln' */
  kind: string;
};

export type LookupBundle = {
  shifts: LookupRow[];
  grades: GradeRow[];
  plants: LookupRow[];
  warehouses: WarehouseRow[];
  source_locations: SourceLocationRow[];
  partner_equipment: PartnerEquipmentRow[];
};

/** Input for `create_production_event`. JS-side keys are camelCase; Tauri's
 * serde rename_all=camelCase deserializes them on the Rust side. */
export type CreateProductionEventInput = {
  recvDate: string;            // ISO YYYY-MM-DD
  prodDate: string | null;     // ISO; null when omitted
  batch: string;               // canonicalized to upper on the Rust side
  shiftCode: string | null;    // 'M' | 'E' | 'N' | null
  gradeCode: string;           // '3X50' etc.
  sourceCode: string;          // 'TNK 1'..'DVO'
  /** Plant override — only honoured when source.kind = 'warehouse_flec' (FLEC).
   * For tank / plant_direct / dvo_container sources, the Rust side forces the
   * plant from the source per §7.2 and ignores this field. */
  plantCodeOverride: string | null;
  warehouseCode: string | null;
  dispositionRaw: string;      // 'FLEC' | 'C1'..'C4' | 'RK1'..'RK4'
  weightKg: number;
  flecCount: number | null;
  whseSide: string | null;     // 'LS' | 'RS' | null
  flecStat: string | null;
  dvoBatchId: number | null;
  notes: string | null;
};

export type SyncSummary = {
  pushed: number;
  /** ISO 8601 timestamp the sync ran. */
  last_synced_at: string;
};

export type SyncStatus = {
  /** ISO 8601 timestamp of the last successful sync; null until the first. */
  last_synced_at: string | null;
  /** Count of locally-edited rows still waiting to push. */
  pending_count: number;
};

// ---------------------------------------------------------------------------
// Step 5 — warehouse + DVO ledger types.
// ---------------------------------------------------------------------------

export type RunBalComponents = {
  opening: number;
  flec_in_to_date: number;
  flec_out_to_date: number;
};

export type FlecLedgerOpening = {
  /** Rust enum serialized as the variant name string: 'G3x50' | 'G2x6' | 'G3p5' | 'G4x8' */
  grade: string;
  /** 'Ls' | 'Rs' */
  side: string;
  flec_count: number;
  period_start_date: string; // ISO
};

export type FlecLedgerRow = {
  event_id: number;
  unique_tag: string;
  recv_date: string;
  prod_date: string | null;
  source: string; // SourceCode variant name
  grade: string; // Grade variant name
  side: string | null; // Side variant or null
  /** Disposition is serde-tagged. We model loosely; UI just uses raw_form. */
  disposition: { kind: 'FlecBagging' } | { kind: 'PartnerCrusher'; equipment_code: number } | { kind: 'PartnerKiln'; equipment_code: number };
  kg_in: number | null;
  kg_out: number | null;
  flec_in: number | null;
  flec_out: number | null;
  run_bal_flec: number;
  run_bal_components: RunBalComponents;
};

export type CurrentBalance = {
  grade: string;
  side: string;
  flec_count: number;
  components: RunBalComponents;
};

export type FlecLedger = {
  warehouse: string;
  start_date: string;
  opening_balances: FlecLedgerOpening[];
  rows: FlecLedgerRow[];
  current_balances: CurrentBalance[];
  unsided_event_count: number;
};

export type DvoBatchSummary = {
  id: number;
  code: string;
  year: number;
  start_month: number;
  /** 'LEFT' | 'RIGHT' */
  side: string;
  /** 'open' | 'closed' */
  status: string;
  opened_at: string;
  closed_at: string | null;
  receipt_count: number;
  outflow_count: number;
  frozen_transit_loss: number | null;
  frozen_yield_loss: number | null;
};

export type WarehouseSummary = {
  code: string;
  default_unit: string; // 'flec_count' | 'kg'
  total_flec: number | null;
  last_event_date: string | null;
  event_count: number;
};

export type DvoReceiptRow = {
  id: number;
  recv_date: string;
  gothong_slip: string | null;
  sack_count: number | null;
  dvo_declared_weight_kg: number;
  cebu_declared_weight_kg: number;
  at_cebu_cy: boolean;
};

export type DvoOutflowRow = {
  event_id: number;
  unique_tag: string;
  recv_date: string;
  prod_date: string | null;
  disposition: FlecLedgerRow['disposition'];
  weight_kg: number;
};

export type DvoLedgerEvent =
  | { kind: 'Receipt'; receipt: DvoReceiptRow; run_bal_kg: number }
  | { kind: 'Outflow'; outflow: DvoOutflowRow; run_bal_kg: number };

export type LossMetric = {
  value: number;
  numerator_kg: number;
  denominator_kg: number;
  frozen: boolean;
};

export type DvoBatchLedger = {
  batch: {
    id: number;
    code: string;
    year: number;
    start_month: number;
    side: string;
    status: string;
    opened_at: string;
    closed_at: string | null;
  };
  receipts: DvoReceiptRow[];
  outflows: DvoOutflowRow[];
  interleaved: DvoLedgerEvent[];
  transit_loss: LossMetric;
  yield_loss: LossMetric;
};

export type ProductionEventRow = {
  id: number;
  recv_date: string;
  prod_date: string | null;
  batch: string;
  shift_code: string | null;
  grade_code: string;
  plant_code: string | null;
  warehouse_code: string | null;
  source_code: string;
  weight_kg: number;
  disposition_kind: string;
  partner_equipment_code: string | null;
  flec_count: number | null;
  whse_side: string | null;
  unique_tag: string;
  notes: string | null;
  created_at: string;
};
