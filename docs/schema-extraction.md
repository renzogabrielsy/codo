# Schema extraction — `2025 CI PRODUCTION V2.xlsb`

> Step 1 deliverable per `PROJECT_BRAIN.md` §9.
> Source workbook: `docs/reference/2025 CI PRODUCTION V2.xlsb` as of 2026-05-07.
> No code is produced from this document. Code starts in Step 2.
>
> **Method note.** This pass was done with [`pyxlsb`](https://pypi.org/project/pyxlsb/), which exposes cell *values* but not formula text. The formula classes documented below are inferred from (a) the value patterns observed in the workbook, (b) `PROJECT_BRAIN.md` §6 which captured these classes from a prior survey pass, and (c) the spec in `docs/reference/charcoal-context-extraction-task.md` Part 2. If we later decide we need formulas verbatim — for example to verify the exact `RUN BAL` SUMIFS shape — install LibreOffice (`brew install --cask libreoffice`) and convert the `.xlsb → .xlsx` so `openpyxl` can read formula text. For everything in this document the value-only pass is sufficient.

---

## §1 — Executive summary

The workbook is a single-operator's hand-built ERP for a charcoal plant: one append-only event log (`Production`) plus five derived warehouse views (`PC WHSE 1/2/5/7`, `PC W3`) plus two plant-level rollups (`W6 Summary`, `W7 Summary`). Four further sheets (`DVO IN/OUT`, `PC W3 - DVO`, `PC WA7 - DVO`) are a parallel ICTC-Davao logistics ledger that does not reference the `Production` table — out of scope for codo's first build.

**The core pattern.** Operators type one row per produced batch into `Production`. Every other in-scope sheet is computed from that table:
- Each `PC WHSE *` sheet picks its warehouse via a single cell (`C2`), filters `Production` to rows that match, spills the matched UNIQUE TAGs starting at `A15`, and then `XLOOKUP`s every other column off that spill. The running balance is a windowed `SUMIFS` keyed on `(grade, side)`.
- Each plant summary is a `GROUP BY plant, prod_date, ccc_or_flec` of `Production` with a daily-detail right side and a monthly-rollup left side.

**Headline translation strategy for codo.** The event log becomes `production_event` in libSQL — one row per `Production` row — with surrogate `id INTEGER PRIMARY KEY`, every categorical resolved to a foreign key against a lookup table, the `UNIQUE TAG` retained as a computed column for audit/export but not used as a key. The five `PC WHSE *` sheets become **one Rust function** `warehouse_ledger(warehouse_id, start_date)` — the windowing logic is awkward in pure SQL but trivial in code, and we want it lock-in tested. The opening-balance block on rows 6–12 of every WHSE sheet becomes a `warehouse_opening_balance` table. The plant summaries become on-demand `GROUP BY` queries — they don't need to be persisted.

**The one number to take away.** The workbook has 769 non-empty production rows across roughly six months (2025-11-28 → 2026-05-03). One row pair has a duplicate `UNIQUE TAG`; 132 rows use `"W7"` in the `WHSE` column where `"WHSE 7"` is the canonical form, and 120 rows for `WHSE 3` use `"WHSE 3"` while the `PC W3` sheet looks for `"W3"`, which is why `PC W3` is currently empty. These are not edge cases — they are the canonicalization debt the new schema needs to fix at write-time.

---

## §2 — The `Production` master table — column dictionary

Headers live in row 1. Rows 2–6 are a **legend** (sample valid values, not data — the `UNIQUE TAG` cell is empty for those rows). Real data begins at row 12 and runs to row 1165. Across the legend gap and trailing blanks, **769 rows are non-empty**.

There are 15 named columns (col 0–14) plus a trailing unnamed column 15 that is empty across the file.

| Col # | Header | Type | Source | Observed values / range | Notes |
|---|---|---|---|---|---|
| 0 | `CCC RECV` | date (Excel serial) | user-typed | 45992–46145 (2025-12-01 → 2026-05-03) | The date the CCC raw material was received. Almost always equal to or one day after `PROD DATE`. **Q2 — confirm semantic.** |
| 1 | `PROD DATE` | date (Excel serial) | user-typed | 45989–46145 (2025-11-28 → 2026-05-03) | Frequently blank on DVO-bound rows (when `PLANT='DVO'`). |
| 2 | `BATCH` | text | user-typed | NOVEMBER, DECEMBER, JANUARY, FEBRUARY, MARCH, APRIL, MAY | One value per calendar month, matching `PROD DATE`'s month. **Strongly suggests `BATCH = month-of(prod_date)`. Q1 — confirm.** |
| 3 | `SHIFT` | enum | user-typed | M (711×), `M,` (1×, typo), ` M` (1×, leading-space typo). Legend (rows 2–4) lists `M`/`E`/`N` but no `E` or `N` rows exist in real data. | Must canonicalize trim+upper. **Q — does the plant actually run E/N shifts at all today?** |
| 4 | `GRADE` | enum | user-typed | `3X50` (624), `2X6` (112), `3.5` (33). Legend also shows `4X8`. | Note `3.5` is stored as numeric `3.5`, not text — type-coerce on read. |
| 5 | `PLANT` | enum | user-typed | `W6` (371), `W7` (188), `DVO` (120), `W6 / W7` (87), `W6 /W7` (1, typo), `W` (1, typo), `37.0` (1, numeric typo) | The DVO plant code matters: it tags rows that are direct outbound shipments to Davao. Canonicalize `W6 / W7` and `W6 /W7` to one form. |
| 6 | `WHSE` | enum | user-typed | `W6` (302), `WHSE 7` (179), `W7` (132), `WHSE 3` (120), `WHSE 5` (31), `WHSE 1` (5) | **`W7` and `WHSE 7` are the same warehouse** (132 + 179 = 311 rows). `W6` and `WHSE 6` would be the same too — but only `W6` is observed. Same shape for `W3`/`WHSE 3` (only `WHSE 3` observed). The PC WHSE sheets do NOT canonicalize on read, so `W7`-tagged production rows do not appear in `PC WHSE 7`. **Real bug.** |
| 7 | `SRC` | enum | user-typed | `FLEC` (165), `W7` (146), `TNK 1` (137), `DVO` (120), `TNK 2` (101), `TNK 3` (50), `W6` (27), `TNK 4` (16) | The source of the input material. `FLEC`-source rows are inflows to a destination warehouse (see §4). `TNK 1–4` are tanks. `W6`/`W7` as SRC means "transferred from the other plant." |
| 8 | `WT` | numeric (kg) | user-typed | ~2,400–26,200 kg | The weight of this batch. |
| 9 | `CCC / FLEC` | enum | user-typed | `C1` (345), `FLEC` (238), `C2` (76), `RK4` (58), `RK3` (26), `RK2` (24), `RK1` (1), `FLEC ` (1, trailing-space typo). Legend also shows `C3`, `C4`. | **The IN/OUT discriminator** — `FLEC` means the row is an inflow to a warehouse; any `C*` or `RK*` value means it is an outflow. See §4 for the inference rule. |
| 10 | `FLEC AMT` | numeric (count) | user-typed | 12–38 (typical) | Number of FLEC bags / units for this batch. Populated only when `CCC / FLEC = FLEC`. The `RUN BAL` formulas track FLEC count, not kg. |
| 11 | `WHSE SIDE` | enum | user-typed | `RS` (50), `LS` (39), plus `NOVEMBER2025RIGHT` (15) and `SEPTEMBER2025LEFT` (4) | `LS`/`RS` is the in-scope value. The date-side strings (e.g. `NOVEMBER2025RIGHT`) are batch codes from the out-of-scope ICTC/DVO tracking system that leaked into this column on DVO-bound rows. Canonicalization needs a non-`LS`/`RS` bucket: store as opaque `dvo_batch_code`, surface as a drift entry, defer to the DVO module. |
| 12 | `FLEC STAT` | enum | user-typed | `DONE` (360). No other observed value. | The legend implies a multi-state column but only `DONE` ever appears. Likely values include something like `PENDING` for rows where the FLEC operation hasn't been completed yet. **Q6 — confirm full state set with Renzo.** Treat blank as the implicit non-DONE state for now. |
| 13 | `DVO SIDE` | enum | user-typed | (always blank in observed data) | Column reserved for DVO-side allocation. Never populated. Out of scope; keep the column for parity but don't define states yet. |
| 14 | `UNIQUE TAG` | text (computed) | formula | 764 distinct values across 765 populated rows; **1 duplicate** | A 10-segment hyphen-concatenation built from cols 0–13. See §3 for the field order and the duplicate. |
| 15 | (unnamed) | — | — | always blank | Spillover col. Ignore. |

**Type rule.** Every date column comes off the wire as Excel-serial floats. Convert via `Excel epoch (1899-12-30) + days_since`. `3.5` in `GRADE` is a numeric — coerce text-or-number on read.

**Density.** The 769 populated rows correspond to ~120 production days over six months ≈ 6 batches/day average across all plants. This is small. SQLite will not break a sweat. Indexes are about query shape, not size.

---

## §3 — The `UNIQUE TAG` composite key

### 3.1 Field order

The tag is `CONCATENATE(field, "-")` over these 10 source columns, in this order:

```
[0]  CCC RECV (Excel serial as-is, e.g. "46091")
[1]  PROD DATE (Excel serial as-is)
[2]  BATCH        (e.g. "MARCH")
[3]  SHIFT        (e.g. "M")
[4]  GRADE        (e.g. "3X50")
[5]  PLANT        (e.g. "W6 / W7")
[6]  WHSE         (e.g. "WHSE 7")
[7]  WHSE SIDE    (e.g. "RS", or empty)
[8]  SRC          (e.g. "TNK 2", or empty)
[9]  CCC / FLEC   (e.g. "FLEC" or "C1" or "RK3")
```

Concatenated with `-` between each pair, including when a field is blank. Examples drawn from the live workbook:

```
46091-46091-MARCH-M-3X50-W6-WHSE 7-RS-TNK 2-FLEC          (all 10 fields populated, RS side, FLEC inflow)
45992-45989-NOVEMBER-M-3X50-W6-W6--TNK 2-C1               (WHSE SIDE blank, producing the "--" run)
45997--DECEMBER--3X50-W6 / W7-WHSE 7--FLEC-RK3            (PROD DATE, SHIFT, WHSE SIDE all blank)
46028--JANUARY-M-3.5-W6-W6--W6-FLEC                       (PROD DATE blank)
46089-46089-MARCH-M,-2X6-37-WHSE 7--FLEC-RK4              (SHIFT typo "M," and PLANT typo numeric 37 propagate verbatim)
```

### 3.2 The "--" runs

When any source field is blank, the concatenation produces consecutive `-` separators (`--`, `---`, etc.). The downstream `PC WHSE *` sheets parse the tag by **fixed character offsets** (`TRIM(MID(SUBSTITUTE(tag, "-", REPT(" ", 100)), 901, 100))` to grab the last segment), which is brittle in two ways:
1. If a *trailing* field is blank, the "last segment" is empty and the IN/OUT inference returns "" instead of `FLEC`/`Cn`/`RKn`.
2. If a *leading* field changes width (e.g. an Excel date serial moves from 5 digits to 6), the offset arithmetic drifts.

### 3.3 Uniqueness in practice

`UNIQUE TAG` is **not actually unique** in the workbook today. There is one duplicate:

```
2x  46113-46113-MARCH-M-3X50-W6-WHSE 7-RS-TNK 3-FLEC
```

Two rows with identical CCC RECV / PROD DATE / BATCH / SHIFT / GRADE / PLANT / WHSE / WHSE SIDE / SRC / CCC class produce the same tag. The downstream `UNIQUE(FILTER(...))` formula in `PC WHSE 7!A15` collapses these two rows into one entry, silently losing the second batch's contribution to `KG IN`/`FLEC IN`.

This is the latent data-integrity bug the brief flagged. In a relational schema that uses `unique_tag` as a `UNIQUE` constraint, the second insert would fail loudly — which is what we want.

### 3.4 Why codo drops it as a primary key

- It is a free-text concatenation of categoricals. Any drift in any one component (e.g. `WHSE 7` vs `W7`, `M` vs `M,`, `W6 / W7` vs `W6 /W7`) creates a sibling row that should have been the same. The sister-app's bug ledger documents this exact failure mode under `listing_name`.
- It is not actually unique today — see §3.3.
- The downstream parsing rule (last-hyphen-segment-as-direction) is a string trick that has no place in backend code; the same fact lives in the `CCC / FLEC` column.

**codo's policy.** `production_event.id INTEGER PRIMARY KEY AUTOINCREMENT` is the row identity. `unique_tag TEXT NOT NULL UNIQUE` is *computed at write* from canonicalized component fields, persisted for audit/export/tag-printing parity with the workbook, and never used to join. When a write would produce a duplicate `unique_tag`, the `UNIQUE` constraint surfaces it; that becomes a `drift_log` entry the operator resolves.

---

## §4 — The `PC WHSE *` ledger pattern

All five sheets — `PC WHSE 1`, `PC WHSE 2`, `PC W3`, `PC WHSE 5`, `PC WHSE 7` — are structurally identical. Only `C2` (the warehouse selector) differs. The file has 2,461 rows allocated per sheet but only the populated rows (15–80 typical) hold real ledger data; the rest are reserved spill capacity.

### 4.1 Header block (rows 1–13)

```
B1: "START:"                    | C1: <Excel-date filter>     <-- user input: ledger start date
B2: "WHSE:"                     | C2: <warehouse selector>    <-- user input: warehouse code (e.g. "WHSE 7")
B3: "FLECON"                                                  <-- section label, "FLEC On hand"
B4: "GRADE"  C4: "RS"  D4: "LS"  E4: "STARTING"               <-- block headers
                                  E5: "RS"  F5: "LS"          <-- sub-headers under "STARTING"

B6:B12  GRADE codes (e.g. "3X50", "2X6", "3.5") -- one row per grade tracked in this warehouse
C6:D12  FLECON values per grade × side          -- COMPUTED  (running balance as of START:)
E6:F12  STARTING values per grade × side        -- USER INPUT (period-opening FLEC counts)
```

**Two different things live in this block, easy to confuse.** The `STARTING` block (E6:F12) is the period-opening **FLEC count** the operator types in once per period. The `FLECON` block (C6:D12) is computed: it equals `STARTING + SUM(FLEC IN before START:) − SUM(FLEC OUT before START:)` per (grade, side) — i.e. "what's actually on hand at START: time, after rolling forward". The `RUN BAL` formula (§4.5) seeds from the `STARTING` block, not the `FLECON` block.

**Real values from `PC WHSE 7` today** (START: 2026-03-10, WHSE: WHSE 7):

| GRADE | FLECON RS | FLECON LS | STARTING RS | STARTING LS |
|---|---|---|---|---|
| 3X50 | 58 | 0 | 53 | (blank) |
| 2X6  | 0  | 0 | (blank) | 26 |

**Real values from `PC WHSE 5` today** (START: 2026-03-10, WHSE: WHSE 5):

| GRADE | FLECON RS | FLECON LS | STARTING RS | STARTING LS |
|---|---|---|---|---|
| 2X6  | 0   | 183 | (blank) | (blank) |
| 3X50 | 619 | 421 | (blank) | (blank) |
| 3.5  | 206 | 0   | (blank) | (blank) |

Notice that the `STARTING` columns can be blank — operators sometimes only fill `STARTING` when they want to override the rolled-forward `FLECON` value.

### 4.2 Ledger header (row 14) — 13 columns

```
A14: UNIQUE TAG  B14: RECV DATE  C14: PROD DATE  D14: SRC
E14: GRADE       F14: SIDE       G14: TAG         H14: STATE
I14: KG IN       J14: KG OUT     K14: FLEC IN     L14: FLEC OUT
M14: RUN BAL
```

### 4.3 The spill anchor in `A15`

`A15` is a single dynamic-array formula of the form

```
= UNIQUE( FILTER( PRODUCTION[UNIQUE TAG],
                  PRODUCTION[WHSE] = $C$2,
                  (PRODUCTION[CCC RECV] >= $C$1) + (PRODUCTION[PROD DATE] >= $C$1) > 0,
                  ... ) )
```

(The exact filter predicate could not be read verbatim without LibreOffice; the value-pattern in row 15+ confirms it filters `WHSE = C2` and matches dates `>= C1`. Verify the precise predicate when LibreOffice is available — the second filter clause may use only `CCC RECV` rather than the OR shown above.)

`A16:A<N>` are **static spilled values**, not separate formulas. Crucial for any future xlsx writer: only `A15` is a formula, the rest of column A is the spilled output.

### 4.4 Per-column lookups in `B15:M15`

Every column from B15 onward is a single dynamic-array formula of the same shape:

```
B15:  = XLOOKUP(A15#, PRODUCTION[UNIQUE TAG], PRODUCTION[CCC RECV])
C15:  = XLOOKUP(A15#, PRODUCTION[UNIQUE TAG], PRODUCTION[PROD DATE])
D15:  = XLOOKUP(A15#, PRODUCTION[UNIQUE TAG], PRODUCTION[SRC])
E15:  = XLOOKUP(A15#, PRODUCTION[UNIQUE TAG], PRODUCTION[GRADE])
F15:  = XLOOKUP(A15#, PRODUCTION[UNIQUE TAG], PRODUCTION[WHSE SIDE])
G15:  = E15# & "-" & F15#                          -- composite (grade, side) key
H15:  = IF( <last segment of A15# = "FLEC"> , "IN", "OUT" )
I15:  = IF(H15#="IN",  XLOOKUP(A15#, PRODUCTION[UNIQUE TAG], PRODUCTION[WT]),       "")
J15:  = IF(H15#="OUT", XLOOKUP(A15#, PRODUCTION[UNIQUE TAG], PRODUCTION[WT]),       "")
K15:  = IF(H15#="IN",  XLOOKUP(A15#, PRODUCTION[UNIQUE TAG], PRODUCTION[FLEC AMT]), "")
L15:  = IF(H15#="OUT", XLOOKUP(A15#, PRODUCTION[UNIQUE TAG], PRODUCTION[FLEC AMT]), "")
M15:  = (windowed running balance — see §4.5)
```

The pattern is "for every Production column we want to display, XLOOKUP it back by UNIQUE TAG." Mechanical plumbing — none of these encode business rules, except the IN/OUT split in `H15` and the running balance in `M15`.

### 4.5 IN/OUT inference (load-bearing business rule)

`H15` ("STATE") is computed by parsing the **last hyphen-segment of `UNIQUE TAG`**, using

```
TRIM( MID( SUBSTITUTE(A15#, "-", REPT(" ", 100)), 901, 100 ) )
```

If the result is `"FLEC"`, STATE is `"IN"`; otherwise `"OUT"`. Verified across all 65 ledger rows of `PC WHSE 7`: every tag ending in `-FLEC` has `STATE = IN`; every other ending (`-C1`, `-C2`, `-RK3`, `-RK4`, …) has `STATE = OUT`.

**Business meaning.** A row whose `CCC / FLEC` value is `FLEC` represents finished FLEC product moving INTO a warehouse for storage. A row whose `CCC / FLEC` value is `C1`–`C4` or `RK1`–`RK4` represents charcoal moving OUT of the warehouse to fulfill a CCC quality grade or a RK rework destination.

**codo's implementation.** Do NOT replicate the character-offset string trick. Use the canonicalized `ccc_or_flec` column directly:

```rust
fn direction(event: &ProductionEvent) -> Direction {
    if matches!(event.ccc_or_flec, CccFlec::Flec) {
        Direction::In
    } else {
        Direction::Out
    }
}
```

This makes the rule local, type-checked, and indexable.

### 4.6 The `RUN BAL` formula

`M15:M<N>` is a per-(grade, side) running balance, **in FLEC count units, not kilograms.** The Excel formula is approximately:

```
= <STARTING for this (grade, side)>
  + SUMIFS( $K$15:K15,  $E$15:E15, E15, $F$15:F15, F15 )    -- FLEC IN totaled up to and incl. this row
  - SUMIFS( $L$15:L15,  $E$15:E15, E15, $F$15:F15, F15 )    -- FLEC OUT totaled up to and incl. this row
```

with the `STARTING` value pulled from `E6:F12` by `(grade, side)`.

Verified against `PC WHSE 7` real values: STARTING (3X50, RS) = 53. First ledger row in (3X50, RS) is an IN with FLEC IN = 33. RUN BAL on that row = 86. 53 + 33 = 86 ✓.

Verified against `PC WHSE 5` real values: STARTING (2X6, LS) = 0 (blank → treated as zero). First ledger row in (2X6, LS) is an OUT with FLEC OUT = 8. RUN BAL = -8. 0 - 8 = -8 ✓. (The `FLECON` cell for (2X6, LS) shows 183, which is what the rolled-forward balance ends up at — but the period RUN BAL still seeds from STARTING = 0, not FLECON = 183.)

**One crisp consequence.** The Excel sheet does NOT track a kg running balance. `KG IN` and `KG OUT` are populated per row but never summed forward. If Renzo wants a kg-on-hand view, it does not exist today — Q below.

### 4.7 The `FLECON` block (C6:D12) roll-forward

The values in the `FLECON` block are computed by formulas of the shape

```
C6 = <STARTING for this grade, RS> + <prior-period FLEC IN for this grade, RS> - <prior-period FLEC OUT...>
```

Equivalent to `warehouse_ledger(<warehouse>, <some earlier date>)` evaluated up through the row immediately before `START:`. It is informational — it does not feed `RUN BAL`.

---

## §5 — Plant summary sheets (`W6 Summary`, `W7 Summary`)

Both summary sheets present the same `Production` data twice in two layouts side by side.

**Left side — monthly rollup.** Columns `A:Q`. Headers: `PLANT | BATCH | (5 spacer cols) | C1 | C2 | C3 | C4 | RK1 | RK2 | RK3 | RK4 | FLEC | Grand Total`. One row per (PLANT, BATCH=month). Values are kg sums of `WT` partitioned by `CCC / FLEC` class.

**Right side — daily detail.** Columns `T:AH`. Headers: `DATE | BATCH | TNK | SHIFT | GRADE | C1 | C2 | C3 | C4 | RK1 | RK2 | RK3 | RK4 | FLEC | TTL`. One row per (DATE, BATCH, TNK, SHIFT, GRADE) with daily-total subtotal rows (e.g. `"12/28/25 Total"`) inserted after each date group.

Real example from `W6 Summary`:

```
W6 | NOVEMBER | … | C1: 29369  | … | FLEC: (blank)  | Grand Total: 29369
W6 | DECEMBER | … | C1: 288192 | … | FLEC: 631099   | Grand Total: 919291
```

### 5.1 Aggregation grain

| Side | Grouping | Aggregate |
|---|---|---|
| Left (monthly) | `(PLANT, BATCH)` | `SUM(WT) PARTITION BY CCC/FLEC class` |
| Right (daily) | `(PROD DATE, BATCH, SRC, SHIFT, GRADE)` | `SUM(WT) PARTITION BY CCC/FLEC class` |

`TNK` on the right side is the `SRC` column from `Production`. Subtotal rows (`<date> Total`) are aggregations of all rows for that date.

### 5.2 codo's implementation

These views are **redundant with what `SELECT … GROUP BY plant, prod_date, ccc_or_flec` would return** from `production_event`. Do not persist. Compute on demand:

```sql
-- Daily detail (right side equivalent)
SELECT prod_date, batch, src.code AS tnk, shift.code AS shift, grade.code AS grade,
       SUM(CASE WHEN ccc_or_flec='C1'  THEN weight_kg END) AS c1_kg,
       SUM(CASE WHEN ccc_or_flec='C2'  THEN weight_kg END) AS c2_kg,
       ...
       SUM(weight_kg) AS total_kg
FROM production_event
JOIN source AS src   ON src.id = production_event.src_id
JOIN shift           ON shift.id = production_event.shift_id
JOIN grade           ON grade.id = production_event.grade_id
WHERE plant_id = ? AND prod_date BETWEEN ? AND ?
GROUP BY prod_date, batch, src.code, shift.code, grade.code
ORDER BY prod_date, src.code, shift.code, grade.code;
```

The monthly version is the same with `GROUP BY DATE_TRUNC('month', prod_date)`.

---

## §6 — Proposed relational schema

This is the schema for codo's first migration (`migrations/v1.sql`). Applied to the **remote Turso DB first** (per `PROJECT_BRAIN.md` §5 gotcha #1), then pulled to the local replica on next sync.

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
    id           INTEGER PRIMARY KEY,
    code         TEXT NOT NULL UNIQUE,    -- 'WHSE 1' | 'WHSE 2' | 'W3' | 'WHSE 5' | 'WHSE 7' (all canonical)
    display_name TEXT NOT NULL,
    branch       TEXT NOT NULL CHECK (branch IN ('CI','ICTC'))
);

CREATE TABLE source (
    id           INTEGER PRIMARY KEY,
    code         TEXT NOT NULL UNIQUE,    -- 'TNK 1' | 'TNK 2' | 'TNK 3' | 'TNK 4' | 'FLEC' | 'W6' | 'W7' | 'DVO'
    display_name TEXT NOT NULL,
    kind         TEXT NOT NULL CHECK (kind IN ('tank','plant_transfer','flec','dvo'))
);
```

Seeded by `seed.sql`. Adding a new value happens via migration (for now — could become a UI write path later).

### 6.2 Core spine — `production_event`

```sql
CREATE TABLE production_event (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,        -- surrogate, NOT unique_tag

    ccc_recv_date   TEXT NOT NULL,                            -- ISO 'YYYY-MM-DD'
    prod_date       TEXT,                                     -- nullable: DVO-bound rows omit it
    batch           TEXT NOT NULL,                            -- canonical month name 'NOVEMBER'..'DECEMBER'
                                                              -- (kept TEXT pending Q1 confirm; if it
                                                              --  truly tracks month-of(prod_date) we may
                                                              --  drop and derive)

    shift_id        INTEGER REFERENCES shift(id),             -- nullable: many DVO rows have no shift
    grade_id        INTEGER NOT NULL REFERENCES grade(id),
    plant_id        INTEGER NOT NULL REFERENCES plant(id),
    warehouse_id    INTEGER NOT NULL REFERENCES warehouse(id),
    src_id          INTEGER NOT NULL REFERENCES source(id),

    weight_kg       REAL NOT NULL CHECK (weight_kg > 0),

    ccc_or_flec     TEXT NOT NULL CHECK (ccc_or_flec IN
                       ('C1','C2','C3','C4','RK1','RK2','RK3','RK4','FLEC')),
    flec_amount     INTEGER CHECK (flec_amount IS NULL OR flec_amount > 0),
                       -- count of FLEC bags; populated only when ccc_or_flec='FLEC'
                       -- AND on outflow rows (XLOOKUP pulls the same column for both sides)

    whse_side       TEXT CHECK (whse_side IS NULL OR whse_side IN ('LS','RS')),
                       -- nullable: not every row is sided (DVO especially)

    flec_stat       TEXT,                                     -- canonicalize at write; closed enum TBD via Q6
    dvo_side        TEXT,                                     -- always blank in observed data; reserved
    dvo_batch_code  TEXT,                                     -- non-LS/RS WHSE SIDE values land here
                                                              -- (e.g. 'NOVEMBER2025RIGHT'); drift_log on write

    unique_tag      TEXT NOT NULL UNIQUE,                     -- computed at write from canonical components
    notes           TEXT,                                     -- free text for operator notes

    dirty           INTEGER NOT NULL DEFAULT 1,               -- cleared after any future push-to-X flow
    created_at      TEXT NOT NULL,                            -- ISO 8601 with offset
    updated_at      TEXT NOT NULL
);

CREATE INDEX idx_pe_warehouse_date     ON production_event (warehouse_id, prod_date);
CREATE INDEX idx_pe_grade_side_date    ON production_event (grade_id, whse_side, prod_date);  -- ledger window
CREATE INDEX idx_pe_ccc_or_flec        ON production_event (ccc_or_flec);                     -- IN/OUT split
CREATE INDEX idx_pe_plant_date         ON production_event (plant_id, prod_date);             -- summary queries
CREATE INDEX idx_pe_unique_tag         ON production_event (unique_tag);                      -- audit lookups
```

**Why every field above.** Each lookup `_id` is a foreign key to a canonical table because every one of these columns has measurable drift in the source workbook (`WHSE 7`/`W7`, `W6 / W7`/`W6 /W7`, `M`/`M,`/` M`). Routing through a lookup table forces canonicalization at the write site. The check constraint on `ccc_or_flec` makes it impossible to insert `FLEC ` with a trailing space — that mistake gets rejected at the DB layer, not at lint time.

### 6.3 Opening balances — `warehouse_opening_balance`

Replaces the user-typed `STARTING` (E6:F12) block on each WHSE sheet.

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

**Note the absence of `opening_kg`.** The Excel sheet does not carry forward a kg balance; only FLEC count. If Q (kg-on-hand desired?) comes back yes, this becomes a v2 migration that adds the column.

### 6.4 Pending events — `production_event_pending` (deferred to Step "later")

The Claude email-funnel agent's writes go here, not into `production_event` directly. Schema is the same shape as `production_event` plus a `source TEXT NOT NULL CHECK (source IN ('email','manual','import'))`, a `source_ref TEXT` for the email Message-Id, and review timestamps. **Defer the migration** until the email-funnel agent is built; including the column up front would tempt premature "pending" code paths. Same reason `production_event_rejected` is deferred.

### 6.5 Support tables

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
    kind        TEXT NOT NULL,        -- 'unique_tag_collision' | 'whse_side_non_canonical' | ...
    target_id   INTEGER,
    expected    TEXT,
    actual      TEXT,
    message     TEXT,
    resolved_at TEXT,
    resolved_by TEXT
);
```

### 6.6 The ledger function (Rust, not SQL)

The five `PC WHSE *` sheets are replaced by **one** function:

```rust
pub fn warehouse_ledger(
    conn: &libsql::Connection,
    warehouse_id: i64,
    start_date: NaiveDate,
) -> Result<Vec<LedgerRow>, Error>
```

Returning rows shaped like row 14+ of the workbook ledger:

```rust
pub struct LedgerRow {
    pub unique_tag: String,
    pub recv_date:  NaiveDate,
    pub prod_date:  Option<NaiveDate>,
    pub src:        String,
    pub grade:      String,
    pub side:       Option<String>,    // Some("LS"|"RS"), or None when whse_side is null
    pub tag:        String,            // "<grade>-<side>" composite
    pub state:      Direction,         // In | Out
    pub kg_in:      Option<f64>,
    pub kg_out:     Option<f64>,
    pub flec_in:    Option<i64>,
    pub flec_out:   Option<i64>,
    pub run_bal:    i64,               // FLEC count, signed
}
```

Algorithm:

1. Load `warehouse_opening_balance` rows for `(warehouse_id, period_start_date <= start_date)`. The latest period per `(grade, side)` wins.
2. Load `production_event` rows for `(warehouse_id, prod_date >= start_date)`, ordered by `prod_date, id`.
3. Walk the rows. For each row, compute direction from `ccc_or_flec`. Update an in-memory `HashMap<(grade_id, side), i64>` running-balance map. Emit one `LedgerRow` per input row.
4. Return the `Vec`.

Lock-in test: a `tmp_db` with 5 `production_event` rows + 1 `warehouse_opening_balance` row, asserting the exact `run_bal` series. `PROJECT_BRAIN.md` §4.6 has this as a Step 5 deliverable.

### 6.7 Cell-by-cell mapping table (workbook → schema)

| Workbook artifact | Becomes |
|---|---|
| `Production!A1:O1` (headers) | column headers in `production_event` (renamed per §6.2) |
| `Production!A12:O<N>` (data) | rows in `production_event` |
| `Production!O<r>` (UNIQUE TAG) | `production_event.unique_tag` (computed column) |
| `Production` rows 2–6 (legend) | seed rows in lookup tables (`shift`, `grade`, lookup CCC class via CHECK) — discarded after seeding |
| `PC WHSE *!C1` (`START:` filter) | URL/form param to `warehouse_ledger(start_date)` |
| `PC WHSE *!C2` (`WHSE:` selector) | URL/form param `warehouse_id` |
| `PC WHSE *!E6:F12` (`STARTING` block) | rows in `warehouse_opening_balance` |
| `PC WHSE *!C6:D12` (`FLECON` block) | computed view: `warehouse_ledger(warehouse_id, period_start)` summed up to start_date |
| `PC WHSE *!A15:M<N>` (ledger spill) | output of `warehouse_ledger(warehouse_id, start_date)` |
| `W6 Summary` / `W7 Summary` | `GROUP BY` query in §5.2 |

Anything not in this table is either spreadsheet plumbing (the per-column `XLOOKUP` formulas) or out of scope (DVO sheets — see §9).

---

## §7 — Business rules catalog

Each rule is tagged:
- `[ENFORCED IN SHEET]` — the workbook would refuse / silently fail without it
- `[CONVENTION ONLY]` — the rule is held by the operator, not by the workbook
- `[INFERRED — CONFIRM]` — derived from observed patterns, needs Renzo's sign-off

The third column proposes where codo enforces it.

| # | Rule | Tag | codo enforcement |
|---|---|---|---|
| 1 | `CCC / FLEC = FLEC` ⇔ inflow; any `Cn`/`RKn` ⇔ outflow | `[ENFORCED IN SHEET]` (via `H15` formula) | application logic; `direction(event)` helper |
| 2 | `unique_tag` should be unique across rows | `[CONVENTION ONLY]` (1 dup observed) | `UNIQUE` constraint on `production_event.unique_tag` |
| 3 | `unique_tag = CCC RECV-PROD DATE-BATCH-SHIFT-GRADE-PLANT-WHSE-WHSE SIDE-SRC-CCC/FLEC` (10 segments, hyphen-joined, blanks become empty) | `[ENFORCED IN SHEET]` (via concat formula in `Production!O`) | `compute_unique_tag(event)` at every write, in transaction |
| 4 | A row is owned by exactly one `(warehouse, side)` | `[CONVENTION ONLY]` | application validation; `whse_side` may be NULL when row is non-sided |
| 5 | Running balance is per-`(warehouse, grade, side)`, in FLEC count units | `[ENFORCED IN SHEET]` (via `M15` SUMIFS) | `warehouse_ledger` Rust function, lock-in tested |
| 6 | Opening balances roll forward via `FLECON` block, but `RUN BAL` formula seeds from `STARTING` block (E6:F12), not `FLECON` (C6:D12) | `[ENFORCED IN SHEET]` (formula chain) | `warehouse_opening_balance.opening_flec_count` is the authoritative seed; `FLECON` view is computed |
| 7 | `BATCH = month-of(prod_date)` (e.g. `prod_date=2026-03-04 → BATCH='MARCH'`) | `[INFERRED — CONFIRM]` (Q1) | once confirmed: drop `batch` column, derive on read |
| 8 | `WHSE 7` and `W7` are the same warehouse; `WHSE 6`/`W6` ditto; `WHSE 3`/`W3` ditto | `[CONVENTION ONLY]` (sheet does NOT canonicalize) | `canonicalize_warehouse(raw)` Rust fn → `warehouse(id)` |
| 9 | `WHSE SIDE` ∈ {`LS`, `RS`}; non-canonical values are ICTC batch codes that should not appear here | `[CONVENTION ONLY]` (workbook accepts anything) | CHECK constraint; non-canonical → `dvo_batch_code` + `drift_log` |
| 10 | `flec_amount` populated only on FLEC rows | `[CONVENTION ONLY]` | application validation: warn (not block) if `ccc_or_flec != 'FLEC' AND flec_amount IS NOT NULL` |
| 11 | `weight_kg > 0` always | `[CONVENTION ONLY]` (no sheet check) | CHECK constraint |
| 12 | `flec_stat` transitions are monotonic toward `DONE` | `[INFERRED — CONFIRM]` (Q6) | `compute_flec_stat_transition(old, new)` once states are known |
| 13 | Production rows with `PLANT='DVO'` are direct shipments to Davao, not warehouse stock movements | `[CONVENTION ONLY]` | `plant.code='DVO'` is a flag the ledger/summary queries respect; these rows DO appear in `production_event` |
| 14 | First-write timestamps (`created_at`, `updated_at`) are set by application, not by user | n/a (workbook has no audit) | trigger or application code |
| 15 | Schema migrations always apply to remote Turso DB first, then propagate to local replica | n/a | migration runner connects to `TURSO_URL` directly (PROJECT_BRAIN §5 gotcha #1) |
| 16 | External-source events go to `production_event_pending`, never auto-promoted | n/a (no current external source) | enforce when email-funnel agent ships; do NOT add the table now |

---

## §8 — Open questions for Renzo

Numbered to align with `PROJECT_BRAIN.md` §11 where possible.

| # | Question | What it blocks | Default if unanswered |
|---|---|---|---|
| Q1 | Is `BATCH` always equal to the calendar month of `PROD DATE`, or are there special-case batch names that don't match? | whether to keep `batch` as a column or derive | keep as `TEXT` column for now |
| Q2 | What does `CCC RECV` semantically mean? Is it the date the CCC raw material was received from the supplier, or the date the finished product was received into the warehouse? | column rename + glossary | leave as `ccc_recv_date`, document as "as recorded in workbook" |
| Q3 | What triggers a row to be `C1` vs `C2` vs `C3` vs `C4` vs `RK1`–`RK4`? Quality grade, process stage, destination type? | `ccc_or_flec` enum semantics + display labels | treat as opaque categorical |
| Q4 | Are there validation rules we can't infer? E.g. `Grade 3X50 only at plant W6`, weight ranges, FLEC AMT-to-WT ratio? | additional CHECK constraints | apply only the rules from §7 |
| Q5 | `WHSE 7` and `W7` — which is the canonical form? My read: `WHSE 7` is canonical, `W7` was a shorthand that crept in. Same call for `WHSE 6 / W6` and `WHSE 3 / W3`. | lookup-table seed values | canonicalize to the prefixed form (`WHSE n`) |
| Q6 | What values does `FLEC STAT` take besides `DONE`? Is there a `PENDING` state, an in-between, or terminal-only? | state machine design (anti-pattern #1: don't transition on partial completion) | enum: `{None_, Done}`. Add states when known. |
| Q7 | Do we want a kg-on-hand running balance, in addition to FLEC count? Excel doesn't track kg run balance today. | extra column on `warehouse_opening_balance` + extra fields on `LedgerRow` | no kg run balance; FLEC count only |
| Q8 | The 1 duplicate UNIQUE TAG (`46113-46113-MARCH-M-3X50-W6-WHSE 7-RS-TNK 3-FLEC`) — is one of these rows a mistake to delete, or are they legitimately two physical batches with the same metadata? | how to handle the migration import | flag in `drift_log` and import both with auto-discriminator suffix on `unique_tag` |
| Q9 | `WHSE SIDE` values like `NOVEMBER2025RIGHT` — these are ICTC batch codes from the DVO module, right? Confirm they should never appear on CI Cebu rows. | `whse_side` CHECK + `dvo_batch_code` field | yes, flag any non-`LS`/`RS` to `drift_log` |
| Q10 | First-launch UX (PROJECT_BRAIN §5 gotcha #2): is it OK to require you to create the empty Turso DB out-of-band, or do you want codo to auto-create it via the Turso Platform API? | Step 2 bootstrap UX | option A — manual create, README documents it |
| Q11 | Production rows with `PLANT='DVO'` — these are the 120 outbound-to-Davao shipments. Should these stay in `production_event` (current proposal) or move to a separate `outbound_shipment` table when we build the DVO module? | future schema | keep in `production_event`; the DVO module can VIEW them |
| Q12 | Should I extract verbatim formulas now via LibreOffice, or is the value-pattern-confirmed picture in §4 sufficient? | depth of this document | sufficient for Step 2; revisit if a lock-in test fails |

Don't surface these to Renzo individually unless a step needs the answer. Q5, Q6, Q9, Q10 likely come up first in Step 2 (repo scaffold). Q1, Q2, Q3 wait until we wire the form. Q7, Q8 wait until the ledger function ships. Q11, Q12 are deferrals.

---

## §9 — Out-of-scope appendix

Four sheets and one whole workbook are intentionally **not** in codo's first build:

### 9.1 The DVO sheets (`DVO IN`, `DVO OUT`, `PC W3 - DVO`, `PC WA7 - DVO`)

These four sheets are a parallel ICTC-Davao logistics ledger. They track Gothong Southern Shipping slips — the trucking-and-shipping company that moves cargo between the Cebu plant and the Davao branch. Headers visible include `GOTHONG TRACKING SLIPS`, `IN COUNT`, `OUT COUNT`, `FOR PULLOUT AT CEBU CY`, `ACTUAL WEIGHT`, `DIFF / LOSS`, and `SKS`. None of the four sheets read from the `Production` table. They form a self-contained system — the references inside the sheets are date-coded warehouse identifiers like `OCTOBER2023LEFT`, `JUNE2025LEFT`, `NOVEMBER2025RIGHT`, mapped to `WHSE 3` and `WHSE A7` (the Davao warehouses) on a per-period basis. The `AVG LOSS` columns track moisture loss percentages on shipments.

The reason a few of these date-coded values bleed through into `Production.WHSE SIDE` (as flagged in §2 col 11 and Q9) is that on DVO-bound rows the operator types the ICTC batch code into the only column that fits. codo handles this by sequestering non-`LS`/`RS` values in the new `dvo_batch_code` column on `production_event` and dropping a `drift_log` entry; the eventual DVO module reads those in.

### 9.2 The RC INVENTORY workbook (`CI RC INVENTORY.xlsb`)

A separate workbook tracking raw-coal inventory (the upstream raw material before charcoal production). It does not reference `Production` at all and represents a different module entirely — warehouse-side raw-coal stock management, distinct from prepared-charcoal output tracking. Reading it is reserved for a future RC-Inventory module. The schema and patterns from this document should largely transfer (event log + per-warehouse ledger), but the column set and business rules are different — that extraction is its own session.

### 9.3 The ICTC Davao plant

Different management, different inventory style. Some of its data appears in this CI workbook only as outbound-shipment rows (`PLANT='DVO'` in `Production`) and as the date-coded codes in the DVO sheets. The actual ICTC operations live elsewhere and will be a separate module.

---

## §10 — What's next

Step 2 (`PROJECT_BRAIN.md` §9) — repo scaffold. The critical inputs from this document for that scaffold:

- `migrations/v1.sql` — the schema in §6.1, §6.2, §6.3, §6.5
- `seed.sql` — the canonical lookup-table values from §2's "Observed values" column, normalized per Q5
- `src-tauri/src/canonicalize.rs` — one `canonicalize_<field>(raw: &str) -> Result<<Field>, CanonError>` per categorical column in §2
- `src-tauri/src/ledger.rs` — the `warehouse_ledger` function spec from §6.6 (stub only at scaffold time; full impl in Step 5)

When Step 2 begins, re-read this document's §6 alongside `PROJECT_BRAIN.md` §3 (stack) and §5 (operational gotchas). The schema is the contract; the migration is the implementation.
