-- Lookup-table seed data. Idempotent — run after every migration so a fresh
-- replica or a freshly-pulled remote ends up with the same canonical values.
--
-- Canonical values come from docs/schema-extraction.md §2 (observed) normalized
-- per the §7 rules: `WHSE 7` is canonical, `W7`/`W6` in the warehouse column
-- are cosmetic noise and NOT seeded as warehouse rows. The kg-per-bag bounds
-- on `grade` come from §7.3 (mean ± rough range; codo tightens after Step 4
-- migration imports real rows).

-- shift -----------------------------------------------------------------------

INSERT OR IGNORE INTO shift (id, code, display_name, sort_order) VALUES
    (1, 'M', 'Morning', 0),
    (2, 'E', 'Evening', 1),
    (3, 'N', 'Night',   2);

-- grade -----------------------------------------------------------------------

INSERT OR IGNORE INTO grade (id, code, display_name, sort_order, expected_kg_per_bag_min, expected_kg_per_bag_max) VALUES
    (1, '3X50', '3 x 50 mesh',  0, 400.0, 700.0),
    (2, '2X6',  '2 x 6 mesh',   1, 400.0, 650.0),
    (3, '3.5',  '3.5 grade',    2, NULL,  NULL),
    (4, '4X8',  '4 x 8 mesh',   3, NULL,  NULL);

-- plant -----------------------------------------------------------------------
--
-- 'W6/W7' is the legacy combined-operation label (87 rows in observed data).
-- Renzo confirmed it as "not sustainable" — migration NULLs it out for FLEC
-- sources and derives from source.plant_id otherwise. Seeded as a plant row
-- only so historical migration paths don't fall over a missing FK.

INSERT OR IGNORE INTO plant (id, code, display_name, branch) VALUES
    (1, 'W6',    'Plant W6 (Cebu, multi-tank)',     'CI'),
    (2, 'W7',    'Plant W7 (Cebu, single-tank)',    'CI'),
    (3, 'W6/W7', 'Plant W6/W7 (legacy combined)',   'CI'),
    (4, 'DVO',   'Davao plant (ICTC)',              'ICTC');

-- warehouse -------------------------------------------------------------------
--
-- WHSE 3 is the only kg-tracked warehouse (DVO product in PP sacks).
-- All others run in flec_count units per §7 rule 7.

INSERT OR IGNORE INTO warehouse (id, code, display_name, branch, default_unit) VALUES
    (1, 'WHSE 1', 'Warehouse 1', 'CI', 'flec_count'),
    (2, 'WHSE 2', 'Warehouse 2', 'CI', 'flec_count'),
    (3, 'WHSE 3', 'Warehouse 3 (DVO)', 'CI', 'kg'),
    (5, 'WHSE 5', 'Warehouse 5', 'CI', 'flec_count'),
    (7, 'WHSE 7', 'Warehouse 7', 'CI', 'flec_count');

-- source_location -------------------------------------------------------------
--
-- §7 rules 23–27 force the SRC↔PLANT pairing. The plant_id below is the
-- canonical pairing; codo uses it to derive production_event.plant_id at
-- write time when the source is a tank, plant_direct, or dvo_container.

INSERT OR IGNORE INTO source_location (id, code, display_name, kind, plant_id) VALUES
    ( 1, 'TNK 1', 'Tank 1 (W6)',           'tank',           1),
    ( 2, 'TNK 2', 'Tank 2 (W6)',           'tank',           1),
    ( 3, 'TNK 3', 'Tank 3 (W6)',           'tank',           1),
    ( 4, 'TNK 4', 'Tank 4 (W6)',           'tank',           1),
    ( 5, 'W7',    'Plant W7 tank',         'tank',           2),
    ( 6, 'W6',    'Plant W6 direct',       'plant_direct',   1),
    ( 7, 'FLEC',  'Bagged inventory',      'warehouse_flec', NULL),
    ( 8, 'DVO',   'Davao container',       'dvo_container',  4);

-- partner_equipment -----------------------------------------------------------

INSERT OR IGNORE INTO partner_equipment (id, code, display_name, kind, sort_order) VALUES
    ( 1, 'C1',  'Crusher 1',     'crusher', 0),
    ( 2, 'C2',  'Crusher 2',     'crusher', 1),
    ( 3, 'C3',  'Crusher 3',     'crusher', 2),
    ( 4, 'C4',  'Crusher 4',     'crusher', 3),
    (11, 'RK1', 'Rotary Kiln 1', 'kiln',    4),
    (12, 'RK2', 'Rotary Kiln 2', 'kiln',    5),
    (13, 'RK3', 'Rotary Kiln 3', 'kiln',    6),
    (14, 'RK4', 'Rotary Kiln 4', 'kiln',    7);
