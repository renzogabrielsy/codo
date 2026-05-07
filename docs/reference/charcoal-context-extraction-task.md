# Charcoal company — Production module context extraction

You will be given a multi-sheet Excel workbook (`2025 CI PRODUCTION V2.xlsb`) used by a prepared-charcoal manufacturing company to log production output and track finished-goods inventory across multiple warehouses. Your job is to produce a backend-schema-oriented context file that documents the workbook's data model precisely enough that the next session can build a Python/database implementation from it without ever opening the spreadsheet.

This document will seed the **first module** of an all-in-one company app: the **Production module**.

The first half of this document gives you a **reference architecture** distilled from a sister inventory app the operator has been running in production for ~6 months (Pokemon TCG inventory + Shopify sync — the patterns transfer; the domain doesn't). Treat that section as the architectural template the operator wants for the new app. **Apply its patterns when you propose the schema in your output.** The second half is the specific extraction task, with explicit format and scope rules.

---

# Part 1 — Reference architecture (apply these patterns in your output)

## What the sister app is

A native macOS desktop tool, single-operator, ~10k+ SKU-level items, offline-first SQLite, push/pull sync to a remote commerce backend. The operator runs it daily. It's been through ~18 schema migrations and a bug ledger of ~18 entries (silent push failures, race windows, partial-state convergence bugs, vocabulary drift between layers). The patterns below are the ones that survived contact with reality.

## Tech stack to recommend

### Backend
- **Python 3.11+** — modern syntax, type hints, no compatibility ceremony
- **Flask** bound to `127.0.0.1` only; serves JSON API + SSE event stream + static frontend
- **SQLite (WAL mode)** — single-file database in `~/Library/Application Support/<App Name>/`, survives app updates
- **Standard library** for everything else: `urllib`, `json`, `subprocess`, `threading`, `socket`. No SQLAlchemy, no ORM, no Celery, no Redis.
- Per-thread SQLite connection pool via `threading.local()`

### Frontend
- **Vanilla HTML/CSS/ES modules** — no build step, no React, no bundler
- **Hash-routing SPA** (`#/production`, `#/inventory`, etc.) — single-page app, ~2k lines plain JS
- **Server-Sent Events** for long-running operation progress
- **IndexedDB** for client-side caching where useful

### Window + distribution
- **PyWebView** wraps Flask in a native Cocoa window
- **Shell-script launcher** wrapped as `.app` bundle. Every launch:
  1. `git pull --quiet origin main` (silent self-update)
  2. Ensure `.venv/` has correct deps
  3. Boot Flask + PyWebView
- Shipping = `git push origin main`. No installer, no notarization dance, no auto-updater.

### Why these choices
- **Why not Django/FastAPI**: overkill for single-operator desktop tool. Flask is 200 lines of boot, fits in your head.
- **Why SQLite not Postgres**: single-operator means single-writer. WAL mode handles concurrent reads. Backups are `cp inventory.db`. No daemon to keep alive.
- **Why no React**: vanilla JS with hash routing is faster to write, faster to deploy, easier to debug.
- **Why git-pull-on-launch**: shipping is just a push. The app self-updates.

## Module skeleton (use this layout)

```
app/
├── <module_name>/
│   ├── main.py              # Flask + PyWebView entry point
│   ├── config.py            # paths, env loader, version
│   ├── db.py                # SQLite schema + migrations + transaction context manager
│   ├── server.py            # Flask app factory + JSON API routes + SSE bus
│   ├── <domain>.py          # one module per domain (production, inventory, …)
│   ├── normalize.py         # canonicalize free-text fields at write time
│   └── static/
│       ├── index.html
│       ├── app.js           # router + el() + api() helpers
│       └── views/
│           ├── production.js
│           ├── inventory.js
│           └── ...
├── build/
│   ├── launcher.sh
│   └── install_launcher.sh
├── docs/
└── tests/
    ├── conftest.py          # `tmp_db` fixture
    └── test_*.py
```

For a charcoal production tracker, the domain modules likely become: `production.py` (master event log writes/reads), `inventory.py` (warehouse balances), `lookups.py` (grades, plants, warehouses, sources), `reports.py` (rollups).

## Database design patterns (load-bearing)

### Append-only migrations

`db.py` defines a list of SQL strings, one per schema version:

```python
_MIGRATIONS: list[str] = [
    # v1 — initial schema
    """CREATE TABLE production_event (...);
       CREATE TABLE warehouse (...);
       CREATE INDEX ...""",
    # v2
    """ALTER TABLE production_event ADD COLUMN flec_stat TEXT;""",
    ...
]

def bootstrap(conn):
    current = _current_version(conn)
    for idx, sql in enumerate(_MIGRATIONS, start=1):
        if idx > current:
            conn.executescript(sql)
            conn.execute("INSERT INTO schema_version VALUES (?)", (idx,))
```

**Never edit a previous migration.** Always append. The migration list is a permanent contract.

### Surrogate ID + natural key columns (don't compose primary keys from free-text)

The Excel workbook uses `UNIQUE TAG` (a 10-field hyphen-concat) as a row identifier. **Don't replicate that in the schema.** Use a surrogate `id INTEGER PRIMARY KEY AUTOINCREMENT` and store the underlying components as separate columns (date, plant, warehouse, grade, etc.). The `UNIQUE TAG` becomes a computed/persisted column for backwards compatibility (display, audit, exports), not the row's identity.

Why: composite-string keys built from free-text fields propagate drift. If anyone enters "Grade A" vs "grade a", you get duplicate rows for what should be the same lot. We learned this the hard way in the sister app — `listing_name` was composed from rarity + variant, casing drift in either component created sibling rows that took 1804 backfill operations to merge.

### Canonicalize-at-write

For any free-text field that becomes a category, lookup, or part of a key:

1. Define `canonicalize_<field>(raw: str) -> str` in `normalize.py`
2. Closed enum lookup: lowercased input → canonical form
3. Apply at **every write site** (insert, update, import, manual entry)
4. Lock-in test: every input variant maps to the canonical
5. Backfill script: walks existing rows, applies canonical, merges duplicates

For the charcoal app, candidates are: GRADE, PLANT, WHSE, SRC, SHIFT, WHSE SIDE, FLEC STAT, DVO SIDE, CCC/FLEC. Anything that gets entered as text and later filtered/grouped/joined.

### State machine pattern (for status flags)

For `FLEC STAT`, `DVO SIDE`, or any multi-state column:

1. Define **closed enum** — `FLEC_STAT_PENDING`, `FLEC_STAT_DONE`, etc.
2. **`compute_<field>_from_input(raw)` helper** — derives canonical state
3. **`compute_<field>_transition(old, new)` helper** — enforces directional rules (terminal states never downgrade)
4. **Test every transition**

Never scatter `if status == 'foo'` conditionals across the codebase. Centralize.

### The "dirty flag" pattern

Every mutable row has `dirty INTEGER NOT NULL DEFAULT 1`. Local edits set 1; sync clears to 0 after pushing successfully. This optimizes any future "push to a downstream system" flow. For a charcoal app with no current downstream sync, you might not need it on day one — but adding it later is a migration; designing it in is free.

### Drift log table

```sql
CREATE TABLE sync_drift_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    detected_at TEXT NOT NULL,
    kind TEXT NOT NULL,
    target_id INTEGER,
    expected TEXT,
    actual TEXT,
    message TEXT
);
```

Append-only. Every silent failure path writes here. UI surfaces with a "Resolve" action. The difference between "things are wrong and you don't know" and "things are wrong and the app is yelling at you."

For charcoal: drift candidates include warehouse balance mismatches between computed and reported, missing FLEC stats on rows where they should be set, lots that don't reconcile across plants.

### Transaction context manager

```python
@contextmanager
def transaction(conn):
    conn.execute("BEGIN")
    try:
        yield conn
        conn.execute("COMMIT")
    except Exception:
        conn.execute("ROLLBACK")
        raise
```

Wrap every multi-statement write. SQLite is fast enough that you should never NOT use a transaction for bulk writes.

### App settings (k/v table)

```sql
CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT);
```

For UI-toggleable knobs, period boundaries, default warehouse, etc.

## Critical real-money invariants

These took the sister app months and a real-money incident to learn. They apply equally to a production tracker:

1. **State changes auto, inventory changes user-confirmed.** If the production sheet says "1000kg moved from W6 to PC WHSE 1," local can update status automatically — but inventory adjustments to the destination warehouse should require operator confirmation (verify the physical receipt matches the bill of lading). Auto-applying inventory changes from external signals corrupts accounting in edge cases (damaged shipments, partial deliveries, miscounts).

2. **Tests cover happy paths; production exposes the topology.** Live operator flows produce data shapes synthetic test setup doesn't replicate. Document a manual-SQL fallback runbook for when the abstraction doesn't fit reality.

3. **Multi-line same-key is a recurring shape.** A single shipment can produce multiple rows of the same (warehouse, grade, side) tuple if logged across separate batches. Code that groups by `(warehouse_id, grade_id, side)` MUST use `defaultdict(list)`, never `{key: line}` dict. The dict form silently collapses duplicates.

4. **Sync orderings are load-bearing.** If you ever add a downstream sync (e.g., to a sales/distribution system): pull before push, always. Push must operate on post-pull state.

5. **Canonicalize at write boundaries; never at read.** Half-fixes leave the other paths bleeding.

## Pre-launch checklist (for any inventory-shaped app)

1. Schema migrations are idempotent and ordered. Running bootstrap() twice is a no-op the second time.
2. All free-text fields that become categories are canonicalized at write time.
3. State machines have closed enums + transition rules + tests for every transition.
4. Every multi-step write is wrapped in `with db.transaction(conn):`.
5. Every external-event detector surfaces in UI for user confirmation; never auto-applies to inventory.
6. Concurrency guards on every fire-and-forget endpoint.
7. Drift log table exists and is populated by every silent-failure path.
8. Test suite covers happy paths AND known topology shapes.
9. Backups: nightly `cp <db_file> <backup-dir>` cron, retain 30 days.
10. Lock-in tests for invariants. These fail loudly when anyone breaks the contract.

## Anti-patterns (don't do these)

1. Don't transition state flags based on partial completion. Mark dirty=0 only after the whole operation lands.
2. Don't compose primary keys from raw free-text. Canonicalize the components first.
3. Don't use `{key: line}` dicts for line-item-like data. Use `defaultdict(list)`.
4. Don't auto-apply external-event side effects to inventory. User confirmation only.
5. Don't trust upstream data verbatim. Pair every ingestion with a canonicalize layer.
6. Don't skip telemetry on silent failures. Drift log entries make invisible bugs visible.
7. Don't optimize prematurely. Vanilla JS, no ORM, no caching layer. Add only when measured.

---

# Part 2 — The extraction task

## Scope — what to analyze

**IN SCOPE** (analyze fully):
- `Production` — master append-only event log
- `PC WHSE 1` — warehouse-1 ledger (derived view)
- `PC WHSE 2` — warehouse-2 ledger (derived view)
- `PC W3` — warehouse-3 ledger (derived view)
- `PC WHSE 5` — warehouse-5 ledger (derived view)
- `PC WHSE 7` — warehouse-7 ledger (derived view)
- `W6 Summary` — W6 plant production rollup
- `W7 Summary` — W7 plant production rollup

**OUT OF SCOPE** (do NOT include in the schema; mention only as "exists, parallel system, not wired to Production"):
- `DVO IN`, `DVO OUT`, `PC W3 - DVO`, `PC WA7 - DVO`

These are a separate Davao-branch logistics ledger (Gothong Southern Shipping tracking slips). They reference each other but do NOT reference the `Production` table.

## What to figure out and document

### 1. The `Production` table — the system of record

- Header row is row 1; data starts row 2; it is a structured Excel Table named `PRODUCTION` (verify the name).
- Document every column: name, data type, allowed/observed values, whether it's a user-entered input or a formula, and what business concept it represents.
- Pay special attention to the `UNIQUE TAG` column. It is a CONCATENATE of 10 fields separated by "-". Document:
  - the exact field order in the tag
  - which fields can be empty (producing "--" runs)
  - the implication for downstream parsers that use position-based MID/SUBSTITUTE extraction (the WHSE sheets parse this tag by fixed character offsets — see §3)
- Identify the natural primary key. Is `UNIQUE TAG` actually unique in practice? Are there composite-key candidates that would be more robust in a relational schema?
- Sample at least 30 rows across the table (start, middle, end) to enumerate observed values for every categorical column: SHIFT, GRADE, PLANT, WHSE, SRC, CCC / FLEC, WHSE SIDE, FLEC STAT, DVO SIDE. These become enums/lookup tables in the backend.
- Document the `FLEC STAT` and `DVO SIDE` columns specifically — they appear to be status flags that gate downstream behavior.

### 2. The five `PC WHSE *` sheets — derived warehouse ledgers

These sheets are NOT data-entry sheets. Below row 14 they are 100% formula-driven from `Production`. Verify this and document:

- The fixed header block (rows 1-13): which cells are user inputs (yellow background — `START` date filter, `WHSE` selector, starting balances per grade × side) vs. which are computed (the running starting balances in C6:D12 that fold prior-period balances with current-period in/out).
- Row 14 column schema (UNIQUE TAG, RECV DATE, PROD DATE, SRC, GRADE, SIDE, TAG, STATE, KG IN, KG OUT, FLEC IN, FLEC OUT, RUN BAL).
- The "spill anchor" formula in A15 — a single `UNIQUE(FILTER(PRODUCTION[UNIQUE TAG], ...))` that materializes every Production row matching this warehouse and date filter. Document the filter predicate exactly. Note that A16 onwards are STATIC values produced by the spill — they are not separate formulas.
- Every column from B15 onward is an `XLOOKUP(A15, PRODUCTION[UNIQUE TAG], PRODUCTION[<col>])` pulling the corresponding field back. List the lookup target for each column.
- The IN/OUT directionality logic: the WHSE sheets infer whether a Production row is an inflow or outflow by parsing the LAST segment of the UNIQUE TAG via `TRIM(MID(SUBSTITUTE(tag, "-", REPT(" ", 100)), 901, 100))`. The result is "FLEC" (inflow → KG IN, FLEC IN populated) or anything else (outflow → KG OUT, FLEC OUT). **Document this rule and explain why a backend implementation should NOT replicate the character-offset trick — it should use the `CCC / FLEC` column directly as the discriminator.**
- The `RUN BAL` formula: a windowed `SUMIFS($K$15:K_n, $E$15:E_n, grade, $F$15:F_n, side) - SUMIFS($L$15:L_n, ...)` plus a starting-balance lookup against the `B6#` spill range. Translate this to plain English: "running balance per (grade, side) within this warehouse, seeded by row-6 starting balances."
- Document the relationship between the `STARTING` block (E6:F12 by grade × RS/LS) and the `FLECON` running-balance block (C6:D12). The C/D values feed the seed of `RUN BAL` for the appended ledger.

### 3. The relational model — explicit mapping for the backend

Produce a relational-schema proposal (no DDL needed; use a list of tables with columns, types, FKs, and indexes). The minimum I expect:

- `production_event` — one row per Production sheet row.
  Columns: id (surrogate), ccc_recv_date, prod_date, batch, shift, grade_id (FK), plant_id (FK), warehouse_id (FK), src_id (FK), weight_kg, ccc_or_flec (enum: 'C1'|'C2'|'FLEC'), flec_amount, whse_side (enum: 'LS'|'RS'), flec_stat, dvo_side, unique_tag (computed/persisted), notes.

- Lookup tables: `grade`, `plant`, `warehouse`, `source`, `shift`, derived from observed values in §1.

- Computed/derived "view" tables that replace each WHSE sheet: `warehouse_ledger_view` parameterized by warehouse_id and start_date, returning the 13-column row schema from §2. Specify whether this should be a SQL view, a materialized view, or an application-layer query — recommend which and why given the access pattern (operators load one warehouse at a time, starting from a chosen date).

- Starting-balance table: `warehouse_opening_balance` (warehouse_id, grade_id, side, period_start_date, opening_kg, opening_flec_count). This replaces the manually-typed E6:F12 block on each WHSE sheet.

For each spreadsheet artifact (cell, range, or formula class) in the IN-SCOPE sheets, map it to either:
- (a) a column in a table, or
- (b) a query/view definition, or
- (c) a UI input that doesn't need to be persisted (e.g., the "START date" filter on the WHSE sheets — that's a URL param or form filter, not a column).

**Apply the patterns from Part 1** when you propose this schema:
- Surrogate `id` + natural columns, NOT `UNIQUE TAG` as primary key
- Lookup tables for every categorical (grade, plant, warehouse, source, shift)
- `dirty` column on `production_event` (cheap to add now, expensive to add later)
- `app_settings` k/v table for the period start date and selected warehouse defaults
- `drift_log` table to surface reconciliation mismatches
- Recommend an **application-layer query** for the WHSE ledger (not a SQL view) so the running-balance windowing logic stays in Python where it's testable and you can reuse the multi-row chaos-sort-style helpers if needed

### 4. Plant rollups: `W6 Summary`, `W7 Summary`

- Document what these aggregate, the granularity (monthly? daily-by-batch?), and which Production columns drive them.
- Identify whether these are duplicative of what a `production_event` SUM/GROUP BY would produce (probably yes), and recommend whether the backend should persist them or compute on demand.

### 5. Business-rule extraction (the most important section)

Beyond the schema, surface the implicit rules the spreadsheet encodes. Examples expected:

- "A FLEC row is an inflow; a non-FLEC row is an outflow."
- "Each Production row belongs to exactly one warehouse and one side (LS/RS); running balances are tracked per (warehouse, grade, side)."
- "The UNIQUE TAG must be unique; if two rows produce the same tag, the WHSE sheet's UNIQUE() will collapse them — this is a latent data-integrity bug to flag."
- "Starting balances are entered once per warehouse and roll forward via the C6/D6 formula chain."
- Any validation rules (allowed grades per plant, allowed sides, weight ranges) you can infer from observed data.

For each rule, mark it `[ENFORCED IN SHEET]`, `[CONVENTION ONLY]`, or `[INFERRED — CONFIRM WITH USER]`.

## How to investigate

1. Read `Production` row 1 (headers) and ~30 rows sampled across the table. Read the formula in `O2` (UNIQUE TAG) verbatim.
2. For ONE representative WHSE sheet (suggest `PC WHSE 7` — it has the richest data), read A1:M16 to capture every formula in the header block and the first two ledger rows. Then read A14:M30 to confirm the spill behavior (formulas only in row 15; static values in rows 16+).
3. Cross-check by reading rows 14:16 of the OTHER four WHSE sheets to confirm they are structurally identical (same formula shape, only the `C2` warehouse selector differs).
4. Read `W6 Summary` and `W7 Summary` headers and a few data rows to pin down their aggregation grain.
5. Do NOT read the DVO sheets beyond confirming they don't reference `PRODUCTION[...]` anywhere. A grep for `PRODUCTION[` across their formulas is sufficient.

## Output format

A single Markdown document with these sections, in this order:

1. **Executive summary** (½ page): what this workbook is, the core pattern (event log → derived warehouse views), and the headline translation strategy for Python/SQL.
2. **The `Production` master table** — column dictionary + enums.
3. **The `UNIQUE TAG` composite key** — exact spec, parsing rules, and why the backend should drop the string concatenation in favor of a surrogate `id` + the underlying columns.
4. **The WHSE ledger pattern** — formula classes, IN/OUT inference, running balance, starting balance seed.
5. **Plant summary sheets** — aggregation grain.
6. **Proposed relational schema** — tables, columns, FKs, indexes, views. Apply the patterns from Part 1.
7. **Business rules catalog** — enforced vs. convention vs. inferred.
8. **Open questions for the human** — things you couldn't determine from the workbook alone (e.g., whether `BATCH` values like "MARCH" are months or batch IDs, what `CCC RECV` semantically means beyond a date, what triggers a row to be `C1` vs `C2` vs `FLEC`).
9. **Out-of-scope appendix** — one paragraph noting the DVO subsystem exists, is parallel, is not wired to Production, and will be handled in a future module.

Length target: ~8–15 pages of Markdown. Be precise, not verbose. Reproduce formulas verbatim only when the formula encodes a business rule that the next implementer must replicate (e.g., the IN/OUT inference, the running balance, the UNIQUE TAG concatenation order). For pure plumbing formulas (the per-column XLOOKUPs), one annotated example plus a list of "all other columns follow the same pattern against PRODUCTION[<col>]" is enough.

---

# Part 3 — How the parts connect

When you produce **§6 (Proposed relational schema)** in your output, your tables should look like:

```
production_event
  id                  INTEGER PRIMARY KEY AUTOINCREMENT  -- surrogate, NOT unique_tag
  ccc_recv_date       TEXT NOT NULL                       -- ISO date
  prod_date           TEXT NOT NULL
  batch               TEXT
  shift_id            INTEGER NOT NULL REFERENCES shift(id)
  grade_id            INTEGER NOT NULL REFERENCES grade(id)
  plant_id            INTEGER NOT NULL REFERENCES plant(id)
  warehouse_id        INTEGER NOT NULL REFERENCES warehouse(id)
  src_id              INTEGER NOT NULL REFERENCES source(id)
  weight_kg           REAL NOT NULL
  ccc_or_flec         TEXT NOT NULL CHECK (ccc_or_flec IN ('C1','C2','FLEC'))
  flec_amount         REAL
  whse_side           TEXT NOT NULL CHECK (whse_side IN ('LS','RS'))
  flec_stat           TEXT       -- canonicalize at write
  dvo_side            TEXT       -- canonicalize at write
  unique_tag          TEXT NOT NULL UNIQUE  -- computed at write from components, kept for audit/export
  notes               TEXT
  dirty               INTEGER NOT NULL DEFAULT 1
  created_at          TEXT NOT NULL
  updated_at          TEXT NOT NULL
  INDEX (warehouse_id, prod_date)
  INDEX (grade_id, whse_side, prod_date)  -- for warehouse_ledger_view's running balance
  INDEX (ccc_or_flec)                      -- IN/OUT discrimination
```

Every lookup table follows the same pattern:

```
grade           id PK, code (canonical), display_name, sort_order
plant           id PK, code, display_name
warehouse       id PK, code, display_name, branch
source          id PK, code, display_name
shift           id PK, code, display_name
```

The `warehouse_ledger_view` is an **application-layer function** in Python, not a SQL view. Pseudo-signature:

```python
def warehouse_ledger(
    conn, warehouse_id: int, start_date: date,
) -> list[dict]:
    """Returns one row per production_event matching this warehouse,
    sorted by prod_date, with computed KG IN / KG OUT / FLEC IN /
    FLEC OUT / RUN BAL columns. Seeded by warehouse_opening_balance."""
```

This is the right place because:
- The running-balance windowing logic is awkward in pure SQL (window functions + UNION with opening balance + per-(grade, side) partitioning) but cleanly expressible in Python
- Operators load one warehouse at a time, starting from a chosen date — the function is naturally parameterized
- You can lock-in test it with a `tmp_db` fixture + 5 production rows, asserting the exact RUN BAL values
- It's reusable from CLI, web UI, and any future export path

The `warehouse_opening_balance` table lets the operator key in starting balances per warehouse × grade × side × period, replacing the manually-typed E6:F12 block. The ledger function joins/unions this with `production_event` to seed the running balance.

When you write **§7 (Business rules catalog)**, structure each rule with:
- The rule, in plain English
- Where it's enforced today (`[ENFORCED IN SHEET]` / `[CONVENTION ONLY]` / `[INFERRED — CONFIRM WITH USER]`)
- Where it should be enforced in the backend (CHECK constraint / canonicalize_X function / state machine transition / application-layer validation / drift_log entry on violation)

That last column is what makes the catalog actionable for the next session that builds the implementation.

---

# Footnotes for the operator

This document is the merge of:
- A specific extraction prompt for the charcoal Excel workbook
- An architectural reference distilled from a sister inventory app the operator has been running for ~6 months

Pair this MD with the Excel workbook (`2025 CI PRODUCTION V2.xlsb`) when you submit to the next Claude Code session. The session will read both and produce the schema-extraction document.

For the operator: the bug ledger from the sister app (`project_known_bugs.md`) is the source of truth for "what actually breaks in production." It's not included here to keep this doc focused, but if the next session asks about specific patterns (e.g., "why is multi-line same-key emphasized?"), point them at the ledger.
