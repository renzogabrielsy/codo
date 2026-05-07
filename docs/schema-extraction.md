# Schema extraction — `2025 CI PRODUCTION V2.xlsb`

> Step 1 deliverable per `PROJECT_BRAIN.md` §9.
> Source workbook: `docs/reference/2025 CI PRODUCTION V2.xlsb` as of 2026-05-07.
> No code is produced from this document. Code starts in Step 2.
>
> **Method note.** This document combines two passes:
> 1. A `pyxlsb` value-only survey of the workbook's structure, sample data, and categorical values (formulas were not extracted verbatim — `pyxlsb` doesn't expose them; the brain's documented formula classes were checked against value patterns and confirmed).
> 2. A walkthrough with Renzo that corrected several inferences from the survey pass — most importantly the meaning of the `WHSE`/`SRC`/`CCC/FLEC` columns, the existence of partner crushers and kilns as the real semantics behind `C1..C4` / `RK1..RK4`, the role of CI vs partner in `FLEC` events, and the special status of WHSE 3 / DVO inflows.
>
> If we later need formulas verbatim — for example to verify the exact `RUN BAL` SUMIFS shape — install LibreOffice (`brew install --cask libreoffice`) and convert the `.xlsb → .xlsx` so `openpyxl` can read formula text. For everything in this document the value-only pass is sufficient.

---

## §1 — Executive summary

The workbook is one operator's hand-built ERP for a charcoal plant, layered over six years of evolving conventions. It encodes a **three-flow model** that codo makes explicit:

1. **CI bagging.** CI takes finished charcoal — either from a plant tank or directly from the plant — and packs it into flecon bags. The bags are stored in a real warehouse (WHSE 1, 2, 5, 7), tagged with a side (LS or RS) and a grade. These rows live in the `Production` sheet with `CCC/FLEC = FLEC`. They're inflows to the warehouse balance.

2. **Partner equipment feeds.** A partner company runs four crushers (`C1`–`C4`) and four rotary kilns (`RK1`–`RK4`) downstream of CI. They take CI's charcoal — from a tank, the plant directly, or already-bagged warehouse stock — weigh it into their carts, and report daily what they fed into which piece of equipment. These rows have `CCC/FLEC = C1..C4` or `RK1..RK4`. They're outflows when sourced from a real warehouse (`SRC = FLEC`); they don't touch warehouse balance when sourced from a tank.

3. **DVO sub-system.** The Davao sister plant (ICTC) ships container vans of finished charcoal via Gothong shipping to CI's Cebu site. Containers are weighed at receipt, assigned to a side of WHSE 3 by `BATCH/SIDE` code (e.g. `NOVEMBER2025RIGHT`), and stored in PP sacks. Partner draws from WHSE 3 the same way they draw from a tank — by weight, no sack count tracked. WHSE 3 needs its own ledger flavor (kg, not flec count) and its own inflow source (`dvo_receipt`, not `production_event`). DVO inflows live in a separate `DVO IN` sheet today, copy-pasted from emails.

**Headline translation strategy for codo.**

- The `Production` sheet → `production_event` table. Surrogate `id` PK, every categorical FK to a lookup, the `UNIQUE TAG` retained as a computed column for audit but not used as a key. The columns `CCC RECV` and `CCC / FLEC` get renamed (`recv_date` and split into `disposition_kind` + `partner_equipment_id`) because "CCC" is just the partner's codename with no domain meaning.
- The `DVO IN` sheet → `dvo_receipt` table, with first-class `dvo_batch` records the operator opens and manually closes. Two loss metrics displayed live and frozen on close: `transit_loss` (Davao declared vs Cebu declared) and `yield_loss` (Cebu received vs partner taken).
- The 5 `PC WHSE *` sheets → **two** Rust functions:
  - `warehouse_ledger_flec(warehouse_id, start_date)` for WHSE 1, 2, 5, 7 — flec count units, per `(grade, side)` running balance, seeded by `warehouse_opening_balance` (which the operator can re-set anytime — no "period" abstraction in the UI).
  - `dvo_batch_ledger(dvo_batch_id)` for WHSE 3 — kg units, scoped to one batch, returns receipts + outflows interleaved with running kg balance and live loss metrics.
- The W6/W7 Summary sheets → on-demand `GROUP BY` queries over `production_event`. Don't persist.

**Information density is a guardrail, not a style.** Every UI surface in codo shows derivations alongside results — running balance row shows starting + ins − outs, KPIs come with their input rows, forms preview their derived fields. This is captured in `PROJECT_BRAIN.md` §4.7 and applies to backend functions too: prefer returning inputs alongside outputs.

**The one number to take away.** The workbook has 769 non-empty production rows across roughly six months (2025-11-28 → 2026-05-03 in the project's snapshot). The migration script's most important jobs are: (a) split `CCC / FLEC` into `disposition_kind` + `partner_equipment_id`, (b) NULL out cosmetic `WHSE = W6/W7` values, (c) parse DVO batch codes out of `WHSE SIDE` into `dvo_batch_id` references, and (d) absorb the `DVO IN` sheet into `dvo_receipt` rows under their corresponding `dvo_batch` records.

---

## §2 — The `Production` master table — column dictionary

Headers live in row 1. Rows 2–6 are a **legend** (sample valid values, not data — the `UNIQUE TAG` cell is empty for those rows). Real data begins at row 12 and runs to row 1165 in the snapshot, with 769 non-empty rows.

| Workbook col | Workbook name | codo schema field | Type | Source | Observed values / range | Notes |
|---|---|---|---|---|---|---|
| 0 | `CCC RECV` | `recv_date` | TEXT (ISO date) | user-typed | 45992–46145 (2025-12-01 → 2026-05-03) | The date the row was logged. Originally meant "date partner sent reports" but Renzo also uses it for CI's own bagging events. Rename strips the legacy "CCC" codename. |
| 1 | `PROD DATE` | `prod_date` | TEXT (ISO date), nullable | user-typed | 45989–46145 | Date the production event/tank-assignment happened. Often blank on DVO outflow rows (partner reports the takeback day, not the production day). |
| 2 | `BATCH` | `batch` | TEXT | user-typed | NOVEMBER, DECEMBER, JANUARY, FEBRUARY, MARCH, APRIL, MAY | Calendar-month batch label. **Not always equal to `month-of(prod_date)`** — at month boundaries CI closes one batch and starts the next on the same physical day. Keep as TEXT, do not derive. |
| 3 | `SHIFT` | `shift_id` | INTEGER FK | user-typed | M (711×), `M,` (1×, typo), ` M` (1×, typo). Legend lists M/E/N but only M observed. | Canonicalize trim+upper. Q: do E/N shifts run today at all? |
| 4 | `GRADE` | `grade_id` | INTEGER FK | user-typed | `3X50` (624), `2X6` (112), `3.5` (33). Legend also lists `4X8`. | Note `3.5` is stored as numeric — type-coerce on read. |
| 5 | `PLANT` | `plant_id` | INTEGER FK | user-typed | `W6` (371), `W7` (188), `DVO` (120), `W6 / W7` (87), plus typos `W6 /W7` (1), `W` (1), `37.0` (1) | Canonicalize space-noise. `DVO` here means "Davao plant" — appears on outflow rows from WHSE 3. |
| 6 | `WHSE` | `warehouse_id` | INTEGER FK, **nullable** | user-typed (auto-fill on newer rows) | Real values: `WHSE 1` (5), `WHSE 2` (0), `WHSE 3` (120), `WHSE 5` (31), `WHSE 7` (179). Cosmetic noise: `W6` (302), `W7` (132). | The destination warehouse for the event. Cosmetic `W6`/`W7` values are pre-auto-fill noise from older rows — Renzo confirmed they should logically be NULL. **Migration NULLs them out.** |
| 7 | `SRC` | `source_location_id` | INTEGER FK | user-typed | `FLEC` (165), `W7` (146), `TNK 1` (137), `DVO` (120), `TNK 2` (101), `TNK 3` (50), `W6` (27), `TNK 4` (16) | The truthful source-location field. Each value maps to a `source_location` row with a `kind`: `tank` (TNK 1..4), `plant_direct` (W6 direct, W7 tank), `warehouse_flec` (FLEC = from already-bagged stock), `dvo_container` (DVO). |
| 8 | `WT` | `weight_kg` | REAL | user-typed | ~2,400–26,200 kg | Weight of this event. Always > 0. |
| 9 | `CCC / FLEC` | `disposition_kind` + `partner_equipment_id` | TEXT enum + INTEGER FK | user-typed | `C1` (345), `FLEC` (238), `C2` (76), `RK4` (58), `RK3` (26), `RK2` (24), `RK1` (1), `FLEC ` (1, typo). Legend also lists `C3`, `C4`. | **The biggest column rewrite.** `FLEC` → `disposition_kind = 'flec_bagging'` (CI bagged into a warehouse), partner_equipment_id NULL. `Cn` → `disposition_kind = 'partner_crusher'` + FK to crusher N. `RKn` → `disposition_kind = 'partner_kiln'` + FK to kiln N. The `FLEC ` trailing-space variant is a canonicalization target. |
| 10 | `FLEC AMT` | `flec_count` | INTEGER, nullable | user-typed | 12–38 typical | Number of flecon **bags** for this event. Populated when `disposition_kind = 'flec_bagging'` and on partner takebacks of bagged stock. The `RUN BAL` formulas track flec count, not kg. |
| 11 | `WHSE SIDE` | `whse_side` *or* `dvo_batch_id` | TEXT or INTEGER FK | user-typed | `RS` (50), `LS` (39), `NOVEMBER2025RIGHT` (15), `SEPTEMBER2025LEFT` (4) | **Polymorphic column.** When the row is on WHSE 1/2/5/7, this is `LS` or `RS`. When the row is on WHSE 3, this carries a DVO batch code in `MONTH(start)YEARSIDE` format (e.g. `NOVEMBER2025RIGHT`); migration parses these into `dvo_batch_id` and sets `whse_side` to NULL. |
| 12 | `FLEC STAT` | `flec_stat` | TEXT, nullable | user-typed | `DONE` (360). No other observed value. | Likely has `PENDING`/blank as the implicit non-DONE state. Q6 open. |
| 13 | `DVO SIDE` | (not modeled in v1) | — | user-typed | (always blank) | Reserved column. Renzo confirmed: was supposed to carry the side info that ended up in `WHSE SIDE` instead. v1 doesn't model it. |
| 14 | `UNIQUE TAG` | `unique_tag` | TEXT UNIQUE (computed) | formula | 764 distinct values across 765 populated rows; **1 confirmed-mistake duplicate** | A 10-segment hyphen-concatenation. See §3 for the field order. The duplicate (rows 844–845: `46113-46113-MARCH-M-3X50-W6-WHSE 7-RS-TNK 3-FLEC` × 2 with identical 1,604 kg / 3 bags) is a confirmed data entry error per Renzo. Migration imports one and logs the other to `drift_log`. |
| 15 | (unnamed) | — | — | — | always blank | Spillover col. Ignore. |

**Type rule.** Every date column comes off the wire as Excel-serial floats. Convert via `Excel epoch (1899-12-30) + days_since`. `3.5` in `GRADE` is a numeric — coerce text-or-number on read.

**Density.** ~120 production days × ~6 batches/day across all plants. SQLite will not break a sweat. Indexes are about query shape, not size.

---

## §3 — The `UNIQUE TAG` composite key

### 3.1 Field order

The tag is `CONCATENATE(field, "-")` over these 10 source columns:

```
[0]  CCC RECV (Excel serial as-is, e.g. "46091")
[1]  PROD DATE (Excel serial as-is)
[2]  BATCH        (e.g. "MARCH")
[3]  SHIFT        (e.g. "M")
[4]  GRADE        (e.g. "3X50")
[5]  PLANT        (e.g. "W6 / W7")
[6]  WHSE         (e.g. "WHSE 7" or cosmetic "W6")
[7]  WHSE SIDE    (e.g. "RS", or empty, or DVO batch code)
[8]  SRC          (e.g. "TNK 2", "FLEC", or empty)
[9]  CCC / FLEC   (e.g. "FLEC" or "C1" or "RK3")
```

Concatenated with `-` between each pair, including when a field is blank. Real examples:

```
46091-46091-MARCH-M-3X50-W6-WHSE 7-RS-TNK 2-FLEC          (CI bagged 33 flec from TNK 2 into WHSE 7 RS)
45992-45989-NOVEMBER-M-3X50-W6-W6--TNK 2-C1               (Partner fed Crusher 1 with TNK 2 charcoal; WHSE=W6 cosmetic)
45997--DECEMBER--3X50-W6 / W7-WHSE 7--FLEC-RK3            (Partner pulled flec from WHSE 7 into RK3; PROD DATE/SHIFT blank)
46028--JANUARY-M-3.5-W6-W6--W6-FLEC                       (CI bagged 3.5 grade direct from W6 plant; legacy missing-WHSE row)
46089-46089-MARCH-M,-2X6-37-WHSE 7--FLEC-RK4              (typos M, and PLANT=37 propagate verbatim)
```

### 3.2 The "--" runs

When any source field is blank, the concatenation produces consecutive `-` separators (`--`, `---`, etc.). The downstream `PC WHSE *` sheets parse the tag by **fixed character offsets** to grab the last segment for IN/OUT inference. codo doesn't replicate that — `disposition_kind` is the discriminator instead.

### 3.3 Uniqueness in practice

`UNIQUE TAG` is **not actually unique** in the workbook today. There is one duplicate, confirmed as a data entry mistake by Renzo:

```
2x  46113-46113-MARCH-M-3X50-W6-WHSE 7-RS-TNK 3-FLEC
    (both rows: WT=1,604 kg, FLEC AMT=3 bags — identical across all 13 logged columns)
```

In a relational schema with a `UNIQUE` constraint on `unique_tag`, the second insert during migration fails. codo handles this by importing the first row and dropping the second into `drift_log` with kind `'unique_tag_collision'` for Renzo to review.

### 3.4 Why codo drops it as a primary key

- It is a free-text concatenation of categoricals. Any drift in any one component (e.g. `WHSE 7` vs `W7`, `M` vs `M,`, `W6 / W7` vs `W6 /W7`) creates sibling rows that should have been the same. The sister-app's bug ledger documents this exact failure mode under `listing_name`.
- It is not actually unique today (see §3.3).
- The downstream parsing rule (last-hyphen-segment-as-direction) is a string trick that has no place in backend code; `disposition_kind` is the same fact, typed.

**codo's policy.** `production_event.id INTEGER PRIMARY KEY AUTOINCREMENT` is the row identity. `unique_tag TEXT NOT NULL UNIQUE` is *computed at write* from canonicalized component fields, persisted for audit/export/tag-printing parity with the workbook, and never used to join.

---

## §4 — The `PC WHSE *` ledger pattern (and what changes for WHSE 3)

All five `PC WHSE *` sheets are structurally identical in the workbook. Only `C2` (the warehouse selector) differs. **But four of them serve a fundamentally different domain than the fifth:** WHSE 1, 2, 5, 7 hold CI Cebu's flec inventory; WHSE 3 holds Davao product. codo splits them into two ledger functions accordingly.

### 4.1 Header block (rows 1–13) — applies to PC WHSE 1, 2, 5, 7

```
B1: "START:"                    | C1: <Excel-date filter>     <-- user input: ledger start date
B2: "WHSE:"                     | C2: <warehouse selector>    <-- user input
B3: "FLECON"                                                  <-- section label, "FLEC On hand"
B4: "GRADE"  C4: "RS"  D4: "LS"  E4: "STARTING"               <-- block headers
                                  E5: "RS"  F5: "LS"          <-- sub-headers under "STARTING"

B6:B12  GRADE codes (e.g. "3X50", "2X6", "3.5") -- one row per grade
C6:D12  FLECON values per grade × side          -- COMPUTED  (running balance as of START:)
E6:F12  STARTING values per grade × side        -- USER INPUT (period-opening flec counts)
```

The `STARTING` block (E6:F12) is the period-opening flec count the operator types in. The `FLECON` block (C6:D12) is computed: it equals `STARTING + sum_in_before_start − sum_out_before_start`. The `RUN BAL` formula seeds from `STARTING`, not `FLECON`.

**Real values from `PC WHSE 7` today** (START: 2026-03-10):

| GRADE | FLECON RS | FLECON LS | STARTING RS | STARTING LS |
|---|---|---|---|---|
| 3X50 | 58 | 0 | 53 | (blank) |
| 2X6  | 0  | 0 | (blank) | 26 |

The `STARTING` block can be re-set whenever the operator wants. **codo's UI strips the "period" abstraction**: the operator just says "as of today, WHSE 7 RS for 3X50 has 53 flec on hand" and codo writes a new opening-balance row dated today. The ledger function uses the most recent opening balance dated on or before the START date.

### 4.2 Ledger header (row 14) — applies to all 5 sheets

```
A14: UNIQUE TAG  B14: RECV DATE  C14: PROD DATE  D14: SRC
E14: GRADE       F14: SIDE       G14: TAG         H14: STATE
I14: KG IN       J14: KG OUT     K14: FLEC IN     L14: FLEC OUT
M14: RUN BAL
```

### 4.3 Spill anchor (`A15`) and column lookups

`A15` is a single dynamic-array formula:

```
= UNIQUE( FILTER( PRODUCTION[UNIQUE TAG],
                  PRODUCTION[WHSE] = $C$2,
                  PRODUCTION[CCC RECV] >= $C$1 ) )
```

(Approximate predicate; the verbatim filter clauses could not be read without LibreOffice. Value patterns confirm `WHSE = C2` and a recv-date cutoff at `C1`.)

`A16:A<N>` are static spilled values, not separate formulas. Crucial: only `A15` is a formula.

Every column from B15 onward is `XLOOKUP(A15#, PRODUCTION[UNIQUE TAG], PRODUCTION[<col>])` — mechanical plumbing. The two columns that encode business rules:

- **`H15` (STATE)** infers IN/OUT from the last hyphen-segment of `UNIQUE TAG` via the `TRIM(MID(SUBSTITUTE(...), 901, 100))` trick. `FLEC` → IN, anything else → OUT. Verified across all PC WHSE 7 rows.
- **`M15` (RUN BAL)** is per-`(grade, side)` running balance in flec count.

### 4.4 IN/OUT inference (load-bearing business rule)

**codo doesn't replicate the substring trick.** Use `disposition_kind` directly:

```rust
fn direction(event: &ProductionEvent) -> Option<Direction> {
    match (event.disposition_kind, event.warehouse_id, event.source_location_kind) {
        // CI bagged into a warehouse → inflow
        (Flec, Some(_warehouse), _) => Some(Direction::In),
        // Partner pulled bagged stock from a warehouse → outflow
        (Crusher | Kiln, Some(_warehouse), Flec) => Some(Direction::Out),
        // Partner pulled from a tank or direct plant → not a warehouse event
        _ => None,
    }
}
```

This makes the rule local, type-checked, and indexable. It also handles the case the substring trick gets wrong: a partner takeback from a tank should NOT count as a warehouse outflow even though `STATE = OUT` in the workbook.

### 4.5 The `RUN BAL` formula

`M15:M<N>` is a per-`(grade, side)` running balance in **flec count units, not kilograms.** The Excel formula is approximately:

```
= <STARTING for this (grade, side)>
  + SUMIFS( $K$15:K15,  $E$15:E15, E15, $F$15:F15, F15 )    -- flec in totaled up to and incl. this row
  - SUMIFS( $L$15:L15,  $E$15:E15, E15, $F$15:F15, F15 )    -- flec out totaled up to and incl. this row
```

with the `STARTING` value pulled from `E6:F12` by `(grade, side)`.

Verified against `PC WHSE 7`: STARTING (3X50, RS) = 53. First ledger row in (3X50, RS) is an IN with FLEC IN = 33. RUN BAL on that row = 86. 53 + 33 = 86 ✓.

**Key consequence.** The Excel sheet does NOT track a kg running balance. `KG IN` and `KG OUT` are populated per row but never summed forward. codo's `warehouse_ledger_flec` returns kg in/out per row (for "always show solution") but does not maintain a running kg balance.

### 4.6 WHSE 3 / DVO is a different ledger

WHSE 3 holds Davao-produced charcoal in PP sacks, dumped into carts on withdrawal. **No flec count, no per-(grade, side) running balance** — instead:

- **One ledger per `dvo_batch`.** Each `BATCH/SIDE` code (e.g. `NOVEMBER2025RIGHT`) is its own self-contained period.
- **Inflows = `dvo_receipt` rows** (per-container records absorbed from the DVO IN sheet). Each row contributes `cebu_declared_weight_kg` to the batch.
- **Outflows = `production_event` rows** with `dvo_batch_id = ?`. Each row contributes `weight_kg` to outflow.
- **Running balance in kg.**
- **Two loss metrics displayed live, frozen on close:**
  - `transit_loss = (sum_dvo_declared − sum_cebu_declared) / sum_dvo_declared` per batch
  - `yield_loss = (sum_cebu_declared − sum_partner_takes) / sum_cebu_declared` per batch
- **Manual close** by operator click (sets `dvo_batch.status = 'closed'`, snapshots the loss metrics into `frozen_transit_loss` / `frozen_yield_loss`).

The legacy `PC W3 - DVO` sheet computes a single `AVG LOSS` per batch but is "month behind because tedious" per Renzo. codo replaces it with the live ledger.

### 4.7 The `FLECON` block roll-forward (PC WHSE 1/2/5/7 only)

The `FLECON` block (C6:D12) values are computed by formulas equivalent to `warehouse_ledger_flec(warehouse, period_start_date)` evaluated up through the row immediately before `START:`. It's informational — it does not feed `RUN BAL`. codo computes it on demand for the same screen as the ledger, surfaced as the "as of START: balance" header above the ledger rows.

---

## §5 — Plant summary sheets (`W6 Summary`, `W7 Summary`)

Both summary sheets present the same `Production` data twice in two layouts side by side.

**Left side — monthly rollup.** Columns `A:Q`. Headers: `PLANT | BATCH | (5 spacer cols) | C1 | C2 | C3 | C4 | RK1 | RK2 | RK3 | RK4 | FLEC | Grand Total`. One row per `(PLANT, BATCH=month)`. Values are kg sums of `WT` partitioned by disposition.

**Right side — daily detail.** Columns `T:AH`. Headers: `DATE | BATCH | TNK | SHIFT | GRADE | C1 | C2 | C3 | C4 | RK1 | RK2 | RK3 | RK4 | FLEC | TTL`. One row per `(PROD DATE, BATCH, SRC, SHIFT, GRADE)` with daily-total subtotal rows.

### 5.1 Aggregation grain

| Side | Grouping | Aggregate |
|---|---|---|
| Left (monthly) | `(PLANT, BATCH)` | `SUM(WT) PARTITION BY disposition` |
| Right (daily) | `(PROD DATE, BATCH, SRC, SHIFT, GRADE)` | `SUM(WT) PARTITION BY disposition` |

`TNK` on the right side is the `SRC` column.

### 5.2 codo's implementation

These views are redundant with `SELECT … GROUP BY` over `production_event`. Do not persist. Compute on demand. The §4.7 information-density rule applies: the rollup screen drills into the daily detail with the source rows visible, so totals are always cross-checkable.

```sql
-- Daily detail (right side equivalent)
SELECT prod_date, batch,
       sl.code AS tnk_or_source, sh.code AS shift, gr.code AS grade,
       SUM(CASE WHEN pe.disposition_kind = 'partner_crusher'
                 AND pq.code = 'C1'   THEN pe.weight_kg END) AS c1_kg,
       SUM(CASE WHEN pe.disposition_kind = 'partner_crusher'
                 AND pq.code = 'C2'   THEN pe.weight_kg END) AS c2_kg,
       ...
       SUM(CASE WHEN pe.disposition_kind = 'flec_bagging' THEN pe.weight_kg END) AS flec_kg,
       SUM(pe.weight_kg) AS total_kg
FROM production_event pe
JOIN source_location AS sl ON sl.id = pe.source_location_id
JOIN shift           AS sh ON sh.id = pe.shift_id
JOIN grade           AS gr ON gr.id = pe.grade_id
LEFT JOIN partner_equipment AS pq ON pq.id = pe.partner_equipment_id
WHERE pe.plant_id = ? AND pe.prod_date BETWEEN ? AND ?
GROUP BY pe.prod_date, pe.batch, sl.code, sh.code, gr.code
ORDER BY pe.prod_date, sl.code, sh.code, gr.code;
```

The monthly version groups by `DATE_TRUNC('month', prod_date)`.

---

## §6 — Proposed relational schema

This is the schema for codo's first migration (`migrations/v1.sql`). Applied to the **remote Turso DB first** per `PROJECT_BRAIN.md` §5 gotcha #1, then pulled to the local replica on next sync.

### 6.1 Lookup tables

```sql
CREATE TABLE shift (
    id           INTEGER PRIMARY KEY,
    code         TEXT NOT NULL UNIQUE,    -- 'M' | 'E' | 'N'
    display_name TEXT NOT NULL,
    sort_order   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE grade (
    id           INTEGER PRIMARY KEY,
    code         TEXT NOT NULL UNIQUE,    -- '3X50' | '2X6' | '3.5' | '4X8'
    display_name TEXT NOT NULL,
    sort_order   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE plant (
    id           INTEGER PRIMARY KEY,
    code         TEXT NOT NULL UNIQUE,    -- 'W6' | 'W7' | 'W6/W7' | 'DVO'
    display_name TEXT NOT NULL,
    branch       TEXT NOT NULL CHECK (branch IN ('CI','ICTC'))
);

CREATE TABLE warehouse (
    id            INTEGER PRIMARY KEY,
    code          TEXT NOT NULL UNIQUE,           -- 'WHSE 1' | 'WHSE 2' | 'WHSE 3' | 'WHSE 5' | 'WHSE 7'
    display_name  TEXT NOT NULL,
    branch        TEXT NOT NULL CHECK (branch IN ('CI','ICTC')),
    default_unit  TEXT NOT NULL CHECK (default_unit IN ('flec_count','kg'))
                                                  -- 'flec_count' for WHSE 1/2/5/7; 'kg' for WHSE 3
);

CREATE TABLE source_location (
    id           INTEGER PRIMARY KEY,
    code         TEXT NOT NULL UNIQUE,            -- 'TNK 1'..'TNK 4' | 'W7' | 'W6' | 'FLEC' | 'DVO'
    display_name TEXT NOT NULL,
    kind         TEXT NOT NULL CHECK (kind IN
                   ('tank','plant_direct','warehouse_flec','dvo_container')),
    plant_id     INTEGER REFERENCES plant(id)     -- NOT NULL for tank/plant_direct
);

CREATE TABLE partner_equipment (
    id           INTEGER PRIMARY KEY,
    code         TEXT NOT NULL UNIQUE,            -- 'C1'..'C4' | 'RK1'..'RK4'
    display_name TEXT NOT NULL,
    kind         TEXT NOT NULL CHECK (kind IN ('crusher','kiln')),
    sort_order   INTEGER NOT NULL DEFAULT 0
);
```

Seeded from `seed.sql` shipped in the repo.

### 6.2 Core spine — `production_event`

```sql
CREATE TABLE production_event (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,

    recv_date            TEXT NOT NULL,                       -- ISO 'YYYY-MM-DD'
    prod_date            TEXT,                                -- nullable: many DVO outflow rows omit
    batch                TEXT NOT NULL,                       -- canonical month name; not strictly = month-of(prod_date)

    shift_id             INTEGER REFERENCES shift(id),        -- nullable
    grade_id             INTEGER NOT NULL REFERENCES grade(id),
    plant_id             INTEGER NOT NULL REFERENCES plant(id),
    warehouse_id         INTEGER REFERENCES warehouse(id),    -- nullable: NULL on tank-stage events
    source_location_id   INTEGER NOT NULL REFERENCES source_location(id),

    weight_kg            REAL NOT NULL CHECK (weight_kg > 0),

    disposition_kind     TEXT NOT NULL CHECK (disposition_kind IN
                            ('flec_bagging','partner_crusher','partner_kiln')),
    partner_equipment_id INTEGER REFERENCES partner_equipment(id),
                            -- NOT NULL when disposition_kind != 'flec_bagging' (enforced in code + trigger)
    flec_count           INTEGER CHECK (flec_count IS NULL OR flec_count > 0),
                            -- bag count for flec_bagging events;
                            -- also populated when partner pulls bagged stock (SRC=FLEC)

    whse_side            TEXT CHECK (whse_side IS NULL OR whse_side IN ('LS','RS')),
                            -- nullable; only populated for WHSE 1/2/5/7 events
    flec_stat            TEXT,                                -- canonicalize at write
    dvo_batch_id         INTEGER REFERENCES dvo_batch(id),
                            -- nullable; non-NULL on DVO outflow rows from WHSE 3

    unique_tag           TEXT NOT NULL UNIQUE,                -- computed at write from canonical components
    notes                TEXT,
    dirty                INTEGER NOT NULL DEFAULT 1,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL
);

CREATE INDEX idx_pe_warehouse_recv     ON production_event (warehouse_id, recv_date);
CREATE INDEX idx_pe_grade_side_recv    ON production_event (grade_id, whse_side, recv_date);  -- ledger window
CREATE INDEX idx_pe_disposition        ON production_event (disposition_kind);
CREATE INDEX idx_pe_plant_prod_date    ON production_event (plant_id, prod_date);             -- summary queries
CREATE INDEX idx_pe_unique_tag         ON production_event (unique_tag);                      -- audit lookups
CREATE INDEX idx_pe_dvo_batch          ON production_event (dvo_batch_id);
```

**Why every field above.** Each lookup `_id` is a foreign key because every one of these columns has measurable drift in the source workbook (`WHSE 7` vs `W7`, `W6 / W7` vs `W6 /W7`, `M` vs `M,`). Routing through a lookup forces canonicalization at the write site. The CHECK on `disposition_kind` makes the `FLEC ` trailing-space mistake impossible to insert.

### 6.3 Opening balances — `warehouse_opening_balance`

Replaces the user-typed `STARTING` (E6:F12) block. Used by `warehouse_ledger_flec` only (WHSE 1/2/5/7). WHSE 3 uses `dvo_batch` instead.

```sql
CREATE TABLE warehouse_opening_balance (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    warehouse_id        INTEGER NOT NULL REFERENCES warehouse(id),
    grade_id            INTEGER NOT NULL REFERENCES grade(id),
    side                TEXT NOT NULL CHECK (side IN ('LS','RS')),
    period_start_date   TEXT NOT NULL,                        -- ISO date
    opening_flec_count  INTEGER NOT NULL DEFAULT 0,
    notes               TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    UNIQUE (warehouse_id, grade_id, side, period_start_date)
);
```

The UI doesn't expose "period" — the operator just writes "as of today, balance is X" and codo creates a new row dated today. The ledger function uses the most recent row dated on or before the START date.

### 6.4 DVO sub-system

```sql
CREATE TABLE dvo_batch (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    code                   TEXT NOT NULL UNIQUE,             -- 'NOVEMBER2025RIGHT'
    warehouse_id           INTEGER NOT NULL REFERENCES warehouse(id),  -- WHSE 3
    start_month            INTEGER NOT NULL CHECK (start_month BETWEEN 1 AND 12),
    year                   INTEGER NOT NULL,
    side                   TEXT NOT NULL CHECK (side IN ('LEFT','RIGHT')),
    status                 TEXT NOT NULL CHECK (status IN ('open','closed')),
    opened_at              TEXT NOT NULL,
    closed_at              TEXT,                             -- NULL until manual close
    closed_by              TEXT,
    notes                  TEXT,
    -- Frozen on close (NULL while open):
    frozen_transit_loss    REAL,
    frozen_yield_loss      REAL,
    created_at             TEXT NOT NULL,
    updated_at             TEXT NOT NULL
);

CREATE INDEX idx_dvo_batch_status ON dvo_batch (status);
CREATE INDEX idx_dvo_batch_warehouse_year ON dvo_batch (warehouse_id, year, start_month);

CREATE TABLE dvo_receipt (
    id                       INTEGER PRIMARY KEY AUTOINCREMENT,
    dvo_batch_id             INTEGER NOT NULL REFERENCES dvo_batch(id),
    recv_date                TEXT NOT NULL,                  -- ISO date
    gothong_slip             TEXT,                           -- the tracking slip identifier (free-text)
    sack_count               INTEGER CHECK (sack_count IS NULL OR sack_count >= 0),
    dvo_declared_weight_kg   REAL NOT NULL CHECK (dvo_declared_weight_kg > 0),
                                                             -- Davao's bill-of-lading number
    cebu_declared_weight_kg  REAL NOT NULL CHECK (cebu_declared_weight_kg > 0),
                                                             -- Cebu's scale at receipt; source of truth
    at_cebu_cy               INTEGER NOT NULL DEFAULT 0,     -- bool: still at Cebu container yard?
    notes                    TEXT,
    created_at               TEXT NOT NULL,
    updated_at               TEXT NOT NULL
);

CREATE INDEX idx_dvo_receipt_batch_date ON dvo_receipt (dvo_batch_id, recv_date);
```

### 6.5 Pending events (deferred to later)

`production_event_pending` mirrors `production_event` plus `source TEXT NOT NULL CHECK (source IN ('email','manual','import'))`, `source_ref TEXT`, review timestamps. **Don't add the migration yet** — the email-funnel agent is a future module and adding the table now would tempt premature "pending" code paths.

### 6.6 Support tables

```sql
CREATE TABLE schema_version (
    version    INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);

CREATE TABLE app_settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE drift_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    detected_at TEXT NOT NULL,
    kind        TEXT NOT NULL,        -- 'unique_tag_collision' | 'whse_side_non_canonical' | 'whse_w6_w7_cosmetic' | ...
    target_id   INTEGER,
    expected    TEXT,
    actual      TEXT,
    message     TEXT,
    resolved_at TEXT,
    resolved_by TEXT
);
```

### 6.7 The ledger functions (Rust, not SQL views)

```rust
pub fn warehouse_ledger_flec(
    conn: &libsql::Connection,
    warehouse_id: i64,
    start_date: NaiveDate,
) -> Result<FlecLedger, Error>

pub struct FlecLedger {
    pub warehouse: Warehouse,
    pub start_date: NaiveDate,
    pub opening_balances: Vec<OpeningBalance>,    // (grade, side) → flec count, with the source row
    pub rows: Vec<FlecLedgerRow>,                  // returns inputs alongside outputs
    pub current_balances: HashMap<(GradeId, Side), i64>,
}

pub struct FlecLedgerRow {
    pub unique_tag: String,
    pub recv_date:  NaiveDate,
    pub prod_date:  Option<NaiveDate>,
    pub source:     SourceLocation,
    pub grade:      Grade,
    pub side:       Option<Side>,
    pub disposition: Disposition,        // shows which crusher / kiln / FLEC
    pub kg_in:      Option<f64>,
    pub kg_out:     Option<f64>,
    pub flec_in:    Option<i64>,
    pub flec_out:   Option<i64>,
    pub run_bal_flec: i64,                // signed; per (grade, side)
    pub run_bal_components: RunBalComponents,  // for "always show solution"
}

pub struct RunBalComponents {
    pub opening: i64,
    pub flec_in_to_date: i64,
    pub flec_out_to_date: i64,
}
```

```rust
pub fn dvo_batch_ledger(
    conn: &libsql::Connection,
    dvo_batch_id: i64,
) -> Result<DvoBatchLedger, Error>

pub struct DvoBatchLedger {
    pub batch: DvoBatch,
    pub receipts: Vec<DvoReceipt>,                 // inflows (kg)
    pub outflows: Vec<DvoOutflowRow>,              // production_event rows with this batch_id
    pub interleaved: Vec<DvoLedgerEvent>,          // both, in date order, with running kg balance
    pub transit_loss: LossMetric,                  // live (or frozen if batch closed)
    pub yield_loss:   LossMetric,
}

pub struct LossMetric {
    pub value: f64,                                // 0.0..1.0 typically; can be negative
    pub numerator_kg: f64,
    pub denominator_kg: f64,
    pub frozen: bool,                              // true once batch is closed
}
```

Both functions return inputs alongside outputs (the §4.7 information-density rule). Both lock-in tested with `tmp_db` fixtures and minimal seed data.

### 6.8 Cell-by-cell mapping table (workbook → schema)

| Workbook artifact | Becomes |
|---|---|
| `Production!A1:O1` (headers) | column headers in `production_event` (renamed per §6.2) |
| `Production!A12:O<N>` (data, 769 rows) | rows in `production_event` |
| `Production!CCC RECV` | `production_event.recv_date` |
| `Production!CCC / FLEC` | split into `production_event.disposition_kind` + `partner_equipment_id` |
| `Production!WHSE` (`W6` / `W7` cosmetic noise) | `warehouse_id` = NULL; drift_log entry kind=`whse_w6_w7_cosmetic` |
| `Production!WHSE SIDE` (DVO batch codes) | parsed → `dvo_batch_id` FK; `whse_side` = NULL |
| `Production!UNIQUE TAG` | `unique_tag` (computed column); duplicate row 844-845 → first imported, second to drift_log |
| `Production` rows 2–6 (legend) | seed rows in lookup tables; discarded after seeding |
| `DVO IN!*` (per-container rows) | `dvo_receipt` rows; batch codes parsed into `dvo_batch` records |
| `PC WHSE *!C1` (`START:` filter) | URL/form param to ledger function |
| `PC WHSE *!C2` (`WHSE:` selector) | URL/form param `warehouse_id` |
| `PC WHSE *!E6:F12` (`STARTING` block) | rows in `warehouse_opening_balance` |
| `PC WHSE *!C6:D12` (`FLECON` block) | computed view: ledger function evaluated up to `START:` |
| `PC WHSE 1/2/5/7!A15:M<N>` (ledger spill) | output of `warehouse_ledger_flec` |
| `PC W3` sheet (currently broken — selector says `W3` but data uses `WHSE 3`) | replaced by `dvo_batch_ledger` per batch; sheet not migrated |
| `PC W3 - DVO`, `PC WA7 - DVO`, `DVO OUT` | not migrated; out of scope |
| `W6 Summary` / `W7 Summary` | `GROUP BY` queries per §5.2 |

---

## §7 — Business rules catalog

Each rule is tagged:
- `[ENFORCED IN SHEET]` — the workbook would refuse / silently fail without it
- `[CONVENTION ONLY]` — held by the operator, not by the workbook
- `[INFERRED — CONFIRM]` — derived from observed patterns, needs Renzo's sign-off

Rules confirmed during the walkthrough are no longer tagged `INFERRED`.

| # | Rule | Tag | codo enforcement |
|---|---|---|---|
| 1 | `disposition_kind = 'flec_bagging'` is a CI act of bagging into a warehouse — inflow when `warehouse_id` is set | confirmed | `direction()` helper (§4.4); CHECK constraint on `disposition_kind` |
| 2 | `disposition_kind ∈ {'partner_crusher','partner_kiln'}` is a partner takeback — outflow only when `source_location.kind = 'warehouse_flec'` AND `warehouse_id` is set | confirmed | `direction()` helper (§4.4) |
| 3 | Partner takebacks from a tank or direct-plant source don't touch any warehouse balance | confirmed | `direction()` returns None for these; ledger function skips them |
| 4 | `unique_tag` should be unique across rows | `[CONVENTION ONLY]` (1 confirmed-mistake dup) | `UNIQUE` constraint; collisions → `drift_log` |
| 5 | `unique_tag = recv_date-prod_date-BATCH-SHIFT-GRADE-PLANT-WHSE-WHSE_SIDE-SRC-CCC/FLEC` (10 segments, hyphen-joined, blanks → empty positions) | `[ENFORCED IN SHEET]` (concat formula in `Production!O`) | `compute_unique_tag(event)` at every write, in transaction |
| 6 | A WHSE 1/2/5/7 row is owned by exactly one `(grade, side)` | `[CONVENTION ONLY]` | application validation; `whse_side` may be NULL when row is non-sided (DVO outflow row, tank-stage row) |
| 7 | WHSE 1/2/5/7 ledger runs in **flec count** units; WHSE 3 ledger runs in **kg** units, scoped per `dvo_batch` | confirmed | two ledger functions (§4); `warehouse.default_unit` enum |
| 8 | Opening balances roll forward via `FLECON` block (informational); `RUN BAL` formula seeds from `STARTING` block | confirmed | `warehouse_opening_balance.opening_flec_count` is authoritative seed; UI computes `FLECON` view on demand |
| 9 | Operator can re-set opening balance any time; UI doesn't expose the "period" abstraction | confirmed (Renzo: "must be open to beginning balances changing a lot") | new opening-balance row dated today; ledger uses most-recent dated ≤ start_date |
| 10 | `BATCH` is a calendar-month label; **does NOT always equal `month-of(prod_date)`** at month-boundary same-day batch transitions | confirmed | TEXT column; do not derive |
| 11 | `WHSE 7` and `W7` are the same warehouse; same for `WHSE 6`/`W6` (latter never used as warehouse) and `WHSE 3`/`W3` | `[CONVENTION ONLY]` (sheet does NOT canonicalize) | `canonicalize_warehouse(raw)` Rust fn; `W6`/`W7` in `WHSE` column → NULL (cosmetic) |
| 12 | `WHSE SIDE` ∈ {`LS`, `RS`} for WHSE 1/2/5/7; on WHSE 3 outflow rows, the column carries a DVO batch code | confirmed | parse at migration; non-canonical → `dvo_batch_id` lookup or `drift_log` |
| 13 | `flec_count` populated on flec_bagging events AND on partner takebacks of bagged stock (SRC=FLEC) | confirmed | application validation; warn if missing on these row types |
| 14 | `weight_kg > 0` always | `[CONVENTION ONLY]` | CHECK constraint |
| 15 | `flec_stat` transitions are monotonic toward `DONE` | `[INFERRED — CONFIRM]` (Q6) | `compute_flec_stat_transition(old, new)` once states are known |
| 16 | DVO batch closure is a **manual** operator action | confirmed (Renzo: "I prefer it to be manual") | "Close batch" button; flips `dvo_batch.status` and snapshots loss metrics |
| 17 | DVO `transit_loss = (sum_dvo_declared − sum_cebu_declared) / sum_dvo_declared` per batch | confirmed | computed live; frozen on close |
| 18 | DVO `yield_loss = (sum_cebu_declared − sum_partner_takes) / sum_cebu_declared` per batch | confirmed | computed live; frozen on close |
| 19 | Davao charcoal in WHSE 3 has no flec count tracked (PP sacks dumped into carts on withdrawal) | confirmed | no `flec_count` on DVO outflow rows; ledger is kg-only |
| 20 | Schema migrations apply to remote Turso DB first | n/a | migration runner connects to `TURSO_URL` directly |
| 21 | External-source events go to `production_event_pending`, never auto-promoted | n/a (no current external source) | enforce when email-funnel agent ships; do NOT add the table now |
| 22 | First-write timestamps (`created_at`, `updated_at`) set by application | n/a (workbook has no audit) | trigger or application code |

---

## §8 — Open questions for Renzo

Most original questions answered during the Step 1 walkthrough. Remaining:

| # | Question | What it blocks | Default if unanswered |
|---|---|---|---|
| Q4 | Validation rules: are there hard rules like "Grade 3X50 only at plant W6" or weight ranges or FLEC AMT-to-WT ratios? | Additional CHECK constraints | Apply only what's already in §7. |
| Q6 | What `flec_stat` values exist besides `DONE`? Workflow state machine? | State machine design | Closed enum `{None_, Done}`. Add states when known. |
| Q10 | First-launch UX: OK to require manual Turso DB creation, or auto-create via Turso Platform API? | Step 2 bootstrap UX | Manual; README documents. |

Everything else from the original Q1–Q12 list was answered:

- **Q1 (BATCH = month?)** — partial: BATCH is a calendar-month label but **not strictly** `month-of(prod_date)` due to same-day month-boundary transitions. Keep TEXT column.
- **Q2 (CCC RECV semantic)** — answered: row logging date; partner reports OR CI's draw-down events.
- **Q3 (C1–C4 / RK1–RK4 meaning)** — answered: partner's 4 crushers and 4 rotary kilns.
- **Q5 (`WHSE 7` vs `W7` canonical form)** — answered: `WHSE 7` is canonical. `W6`/`W7` in `WHSE` column are cosmetic and migrate to NULL.
- **Q7 (kg-on-hand for WHSE 1/2/5/7?)** — answered: not for those; only WHSE 3 runs in kg, per batch.
- **Q8 (duplicate UNIQUE TAG)** — answered: confirmed mistake; migration imports one, drift-logs the other.
- **Q9 (`NOVEMBER2025RIGHT` etc.)** — answered: DVO batch codes; sequester via `dvo_batch_id`.
- **Q11 (DVO outflows separate table?)** — answered: no, they stay in `production_event` with `dvo_batch_id` FK; only DVO inflows get their own table (`dvo_receipt`).
- **Q12 (verbatim formulas needed?)** — answered: no, value-pattern-confirmed picture is sufficient.

---

## §9 — Out-of-scope appendix

### 9.1 The legacy DVO sheets (`DVO OUT`, `PC W3 - DVO`, `PC WA7 - DVO`)

Three sheets in the workbook are **out of scope** even though they relate to DVO — Renzo gave up maintaining them ("month behind because tedious") and codo replaces their function with the live `dvo_batch_ledger`.

- `DVO OUT` — partial outbound logistics ledger Renzo doesn't keep current.
- `PC W3 - DVO` — horizontal-layout per-batch tracker with `MONTH` / `YEAR` / `SIDE` / `START PROD` / `END PROD` / `AVG LOSS` columns per batch code. The `AVG LOSS` value here is replaced in codo by `transit_loss` + `yield_loss` (two distinct losses, not one merged).
- `PC WA7 - DVO` — older version of the same tracker.

The `DVO IN` sheet is **in scope** (becomes `dvo_receipt`).

### 9.2 The RC INVENTORY workbook (`CI RC INVENTORY.xlsb`)

A separate workbook tracking raw-coal inventory (the upstream raw material before charcoal production). Doesn't reference `Production` at all. Reading it is reserved for a future RC-Inventory module. The schema and patterns here largely transfer; column set and business rules are different.

### 9.3 The ICTC Davao plant operations

Different management, different inventory style. Only ICTC's *outbound containers to Cebu* are in scope (via `dvo_receipt`). Their internal operations live elsewhere and will be a separate module.

---

## §10 — What's next

Step 2 (`PROJECT_BRAIN.md` §9) — repo scaffold. Critical inputs from this document:

- `migrations/v1.sql` — the schema in §6.1, §6.2, §6.3, §6.4, §6.6
- `seed.sql` — canonical lookup-table values from §2's "Observed values" column, normalized per §7 rule 11
- `src-tauri/src/canonicalize.rs` — one `canonicalize_<field>(raw: &str) -> Result<<Field>, CanonError>` per categorical column in §2
- `src-tauri/src/ledger.rs` — `warehouse_ledger_flec` + `dvo_batch_ledger` function specs from §6.7 (stub only at scaffold time; full impl in Step 5)
- `src-tauri/src/direction.rs` — the `direction()` helper from §4.4

When Step 2 begins, re-read this document's §6 alongside `PROJECT_BRAIN.md` §3 (stack), §4.7 (information density), and §5 (operational gotchas). The schema is the contract; the migration is the implementation.
