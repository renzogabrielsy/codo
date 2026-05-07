-- codo schema v1 — initial migration.
--
-- Source of truth: docs/schema-extraction.md §6 (the column-by-column treatment)
-- and §7.1 (the validity matrix that this schema enforces in DB-level CHECKs
-- where possible, in app-layer Rust where the rule needs more than one row's
-- worth of context).
--
-- PROJECT_BRAIN.md §5 #1: this file is applied to the REMOTE Turso DB first,
-- then the local libSQL embedded replica picks it up on `db.sync()`. Editing
-- this file after release is FORBIDDEN — append a new migrations/vN.sql instead.

PRAGMA foreign_keys = ON;

-- Lookup tables ---------------------------------------------------------------

CREATE TABLE IF NOT EXISTS shift (
    id           INTEGER PRIMARY KEY,
    code         TEXT    NOT NULL UNIQUE,
    display_name TEXT    NOT NULL,
    sort_order   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS grade (
    id                       INTEGER PRIMARY KEY,
    code                     TEXT    NOT NULL UNIQUE,
    display_name             TEXT    NOT NULL,
    sort_order               INTEGER NOT NULL DEFAULT 0,
    expected_kg_per_bag_min  REAL,
    expected_kg_per_bag_max  REAL
);

CREATE TABLE IF NOT EXISTS plant (
    id           INTEGER PRIMARY KEY,
    code         TEXT    NOT NULL UNIQUE,
    display_name TEXT    NOT NULL,
    branch       TEXT    NOT NULL CHECK (branch IN ('CI', 'ICTC'))
);

CREATE TABLE IF NOT EXISTS warehouse (
    id           INTEGER PRIMARY KEY,
    code         TEXT    NOT NULL UNIQUE,
    display_name TEXT    NOT NULL,
    branch       TEXT    NOT NULL CHECK (branch IN ('CI', 'ICTC')),
    default_unit TEXT    NOT NULL CHECK (default_unit IN ('flec_count', 'kg'))
);

CREATE TABLE IF NOT EXISTS source_location (
    id           INTEGER PRIMARY KEY,
    code         TEXT    NOT NULL UNIQUE,
    display_name TEXT    NOT NULL,
    kind         TEXT    NOT NULL CHECK (kind IN ('tank', 'plant_direct', 'warehouse_flec', 'dvo_container')),
    plant_id     INTEGER REFERENCES plant(id)
);

CREATE TABLE IF NOT EXISTS partner_equipment (
    id           INTEGER PRIMARY KEY,
    code         TEXT    NOT NULL UNIQUE,
    display_name TEXT    NOT NULL,
    kind         TEXT    NOT NULL CHECK (kind IN ('crusher', 'kiln')),
    sort_order   INTEGER NOT NULL DEFAULT 0
);

-- DVO sub-system --------------------------------------------------------------

CREATE TABLE IF NOT EXISTS dvo_batch (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    code                   TEXT    NOT NULL UNIQUE,
    warehouse_id           INTEGER NOT NULL REFERENCES warehouse(id),
    start_month            INTEGER NOT NULL CHECK (start_month BETWEEN 1 AND 12),
    year                   INTEGER NOT NULL,
    side                   TEXT    NOT NULL CHECK (side IN ('LEFT', 'RIGHT')),
    status                 TEXT    NOT NULL CHECK (status IN ('open', 'closed')),
    opened_at              TEXT    NOT NULL,
    closed_at              TEXT,
    closed_by              TEXT,
    notes                  TEXT,
    frozen_transit_loss    REAL,
    frozen_yield_loss      REAL,
    created_at             TEXT    NOT NULL,
    updated_at             TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_dvo_batch_status            ON dvo_batch (status);
CREATE INDEX IF NOT EXISTS idx_dvo_batch_warehouse_year    ON dvo_batch (warehouse_id, year, start_month);

CREATE TABLE IF NOT EXISTS dvo_receipt (
    id                       INTEGER PRIMARY KEY AUTOINCREMENT,
    dvo_batch_id             INTEGER NOT NULL REFERENCES dvo_batch(id),
    recv_date                TEXT    NOT NULL,
    gothong_slip             TEXT,
    sack_count               INTEGER CHECK (sack_count IS NULL OR sack_count >= 0),
    dvo_declared_weight_kg   REAL    NOT NULL CHECK (dvo_declared_weight_kg > 0),
    cebu_declared_weight_kg  REAL    NOT NULL CHECK (cebu_declared_weight_kg > 0),
    at_cebu_cy               INTEGER NOT NULL DEFAULT 0,
    notes                    TEXT,
    created_at               TEXT    NOT NULL,
    updated_at               TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_dvo_receipt_batch_date ON dvo_receipt (dvo_batch_id, recv_date);

-- Core spine ------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS production_event (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,

    recv_date            TEXT    NOT NULL,
    prod_date            TEXT,
    batch                TEXT    NOT NULL,

    shift_id             INTEGER REFERENCES shift(id),
    grade_id             INTEGER NOT NULL REFERENCES grade(id),
    plant_id             INTEGER REFERENCES plant(id),
    warehouse_id         INTEGER REFERENCES warehouse(id),
    source_location_id   INTEGER NOT NULL REFERENCES source_location(id),

    weight_kg            REAL    NOT NULL CHECK (weight_kg > 0),

    disposition_kind     TEXT    NOT NULL CHECK (disposition_kind IN ('flec_bagging', 'partner_crusher', 'partner_kiln')),
    partner_equipment_id INTEGER REFERENCES partner_equipment(id),
    flec_count           INTEGER CHECK (flec_count IS NULL OR flec_count > 0),

    whse_side            TEXT CHECK (whse_side IS NULL OR whse_side IN ('LS', 'RS')),
    flec_stat            TEXT,
    dvo_batch_id         INTEGER REFERENCES dvo_batch(id),

    unique_tag           TEXT    NOT NULL UNIQUE,
    notes                TEXT,
    dirty                INTEGER NOT NULL DEFAULT 1,
    created_at           TEXT    NOT NULL,
    updated_at           TEXT    NOT NULL,

    -- Disposition / partner_equipment coherence (the §7.1 row-level rule that
    -- doesn't need cross-row context lives at the DB).
    CHECK (
        (disposition_kind = 'flec_bagging' AND partner_equipment_id IS NULL)
        OR (disposition_kind IN ('partner_crusher', 'partner_kiln') AND partner_equipment_id IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_pe_warehouse_recv  ON production_event (warehouse_id, recv_date);
CREATE INDEX IF NOT EXISTS idx_pe_grade_side_recv ON production_event (grade_id, whse_side, recv_date);
CREATE INDEX IF NOT EXISTS idx_pe_disposition    ON production_event (disposition_kind);
CREATE INDEX IF NOT EXISTS idx_pe_plant_prod_date ON production_event (plant_id, prod_date);
CREATE INDEX IF NOT EXISTS idx_pe_unique_tag      ON production_event (unique_tag);
CREATE INDEX IF NOT EXISTS idx_pe_dvo_batch       ON production_event (dvo_batch_id);

-- Opening balances ------------------------------------------------------------

CREATE TABLE IF NOT EXISTS warehouse_opening_balance (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    warehouse_id        INTEGER NOT NULL REFERENCES warehouse(id),
    grade_id            INTEGER NOT NULL REFERENCES grade(id),
    side                TEXT    NOT NULL CHECK (side IN ('LS', 'RS')),
    period_start_date   TEXT    NOT NULL,
    opening_flec_count  INTEGER NOT NULL DEFAULT 0,
    notes               TEXT,
    created_at          TEXT    NOT NULL,
    updated_at          TEXT    NOT NULL,
    UNIQUE (warehouse_id, grade_id, side, period_start_date)
);

-- Support tables --------------------------------------------------------------

CREATE TABLE IF NOT EXISTS app_settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS drift_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    detected_at TEXT NOT NULL,
    kind        TEXT NOT NULL,
    target_id   INTEGER,
    expected    TEXT,
    actual      TEXT,
    message     TEXT,
    resolved_at TEXT,
    resolved_by TEXT
);
