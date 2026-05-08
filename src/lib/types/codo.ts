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
