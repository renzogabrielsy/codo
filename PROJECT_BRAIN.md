# CI Production & Inventory App — Project Brain

> Single source of truth for any new Claude Code session working in this repo.
> Last updated: **2026-05-07**.
> If anything in here turns out to be wrong, fix it here first, then keep coding.

---

## §0 — Read this first

You (the new session) are picking up a project that has finished its planning phase. Nothing has been built yet. The next deliverable is a workbook → schema spec doc; after that, the repo scaffold.

Before you do anything else, read in this order:
1. **This file** (`PROJECT_BRAIN.md`) — the consolidated context.
2. **`docs/reference/charcoal-context-extraction-task.md`** — Renzo's original brief for the schema-extraction task. Section 2 is the actual workbook specification.
3. **`docs/reference/architecture-reference.md`** — the patterns from Renzo's sister app (still.hobbies). Don't blindly copy the stack from this file — the stack changed (see §3 here). Do copy the patterns (canonicalize-at-write, drift log, dirty flag, state machines, etc.).
4. **`docs/reference/2025 CI PRODUCTION V2.xlsb`** — the live workbook. Read with `pyxlsb` for values, convert to `.xlsx` via LibreOffice + read with `openpyxl` if you need formulas.
5. **`docs/reference/CI RC INVENTORY.xlsb`** — reference only. Do not build against it yet.

The shortcut goes in your explanation, never in your work. If the task says "read every WHSE sheet's formulas," read every one.

---

## §1 — What we're building (plain English)

Renzo runs a charcoal company in Cebu (**CI**) and a sister branch in Davao (**ICTC**). Right now, his Cebu plant tracks every batch of charcoal it makes in a single Excel workbook. People type rows into a "Production" sheet; five other sheets read those rows back out and compute warehouse balances using a wall of formulas. It works, but:

- He can't show management the numbers without screenshotting the spreadsheet.
- Anyone with the file can break the formulas.
- There's no real audit trail — Excel doesn't tell you who changed a cell at 11:43pm.
- You can't have two people editing it at once.
- It can't talk to anything else (sales system, supplier emails, etc.).

We're replacing it with a **local desktop app** that:

1. Logs production events fast (faster than typing into Excel, with categorical fields canonicalized so "WHSE 7" and "W7" don't both end up as separate values).
2. Keeps a local SQLite database that mirrors to a cloud Postgres-compatible DB (Turso) automatically.
3. Lets a future management dashboard read those numbers from a real URL with logins.
4. Sets the stage for a **Claude-powered email-funnel agent** later — production data emailed in by suppliers gets parsed and queued for Renzo to confirm with one click.
5. Is shipped as a public open-source repo, with no real data ever in the repo, so it can be commercialized later (sell the hosted version, or sell deployment to other charcoal companies).

That's the whole story.

---

## §2 — Scope for the FIRST build

**IN scope (build now):**
- The **Production module** for **CI Cebu only**.
- Local desktop app for daily entry.
- Local SQLite database.
- Cloud sync to Turso.
- Excel migration script to bring in the existing ~1,150 production rows.
- Warehouse ledger view (the formula-heavy derived sheets translated to a Rust function).

**OUT of scope (defer, do not build):**
- **RC Inventory module** — the `CI RC INVENTORY.xlsb` workbook (warehouse-side raw-coal inventory tracking). Will be a future module. Keep it readable as reference but don't write code against it.
- **ICTC Davao plant** — different management, different inventory style. Future module.
- **DVO sheets** in the Production workbook (`DVO IN`, `DVO OUT`, `PC W3 - DVO`, `PC WA7 - DVO`) — these are a parallel Davao-branch logistics ledger (Gothong Southern Shipping tracking) that doesn't reference the `Production` table. Ignore.
- **Management dashboard URL** — the *capability* to expose data is part of the architecture from day one (that's why we sync to Turso), but the actual dashboard is a separate small app built later when Renzo decides who logs in and what they see.
- **Multi-tenant features** — single-operator app. Multi-tenant comes if/when Renzo sells to a second company.
- **Auth** for the desktop app — single operator on a single Mac. No login screen needed.

---

## §3 — Tech stack (verified May 2026)

This stack was verified by a focused web-research pass against the latest releases as of 2026-05-07. Three changes were made from the initial proposal based on that research; reasons noted inline.

| Layer | Pick | Notes |
|---|---|---|
| **Shell** | **Tauri v2** (≥ 2.6 for the updater bug fix) | Native window. ~5–10 MB binaries. Multi-platform (Mac, Windows, Linux). Real signed auto-updater. Mobile possible later via Tauri v2 mobile. |
| **Backend lang** | **Rust** (current stable) | Runs inside Tauri. Talks to libSQL. Renzo isn't expected to write Rust — just to read it. |
| **DB client** | **`libsql` Rust crate** | Turso's official client. Mature in Rust (much more so than its Python bindings). Embedded-replica support is first-class here. |
| **Local DB → cloud sync** | **libSQL embedded replica → Turso cloud** | Local SQLite file IS the replica. Auto-syncs to Turso with `db.sync()`. **Caveat:** Turso has marked embedded replicas as "legacy" in favor of "Turso Sync" on their new Rust engine (still in beta as of May 2026). Plan: ship on embedded replicas now, migrate to Turso Sync when it hits GA. SQL surface is compatible, so it's a swap not a rewrite. |
| **Frontend framework** | **SvelteKit 2 + Svelte 5 (runes)**, in **SPA mode** via `@sveltejs/adapter-static` | No SSR (we're inside a webview). `+layout.ts` must set `ssr=false`, `prerender=false`, `fallback: 'index.html'`. |
| **Styling** | **Tailwind CSS v4** (with `@tailwindcss/vite`) | **Vite plugin order matters:** SvelteKit plugin first, Tailwind second, or class scanning silently breaks. Use `@reference "tailwindcss";` at the top of any `<style>` block that uses `@apply`. |
| **UI components** | **shadcn-svelte** (Tailwind v4 + Svelte 5 fully supported) | Copy-paste components, you own the code. Looks like a real app. |
| **Charts** | **Chart.js + thin Svelte 5 wrapper** *(changed from LayerChart)* | LayerChart 2's Svelte 5 line is still on `2.0.0-next.62` prerelease and has a history of breaking on Svelte minor bumps. Chart.js is boring and bulletproof. |
| **Type bridge (Rust ↔ TS)** | **`tauri-specta`** *(pinned to exact RC version)* | Generates TypeScript types from Rust structs and `#[tauri::command]` handlers. Still on `2.0.0-rc.24` after ~2 years of RCs. **Pin the exact version in both `Cargo.toml` and `package.json`.** Plan B if it breaks: hand-write TS interfaces or switch to `ts-rs`. |
| **Forms** | **`sveltekit-superforms` + `zod`** | Works with Svelte 5 runes; expect to need `$derived` wrappers around form state in a few places. Single-operator app, friction is minor. |
| **State** | **Svelte 5 runes** + native stores | No Redux, no Zustand. Built in is enough. |
| **Excel migration script** | **Standalone Python script** in `/scripts/` | One-shot. Uses `openpyxl` for `.xlsx` (after converting `.xlsb` via LibreOffice headless) or `pyxlsb` for value-only reads. Not part of the binary. |
| **Claude email-funnel agent (later)** | **Separate Python daemon** | Lives outside the desktop app entirely. Talks to the same Turso DB via the libSQL HTTP API. Uses the Anthropic Python SDK. Decoupling is a feature, not a workaround. |
| **Build / packaging** | **Tauri's built-in bundler** | Outputs `.dmg` (macOS), `.msi` (Windows), `.AppImage` (Linux). Signed installers. Auto-update via `tauri-plugin-updater` v2. |
| **Secrets at runtime** | **OS keyring** via `tauri-plugin-stronghold` or the `keyring` Rust crate | **Critical:** Turso URL + token CANNOT live in a baked-in `.env`. They're prompted on first launch and stored in macOS Keychain / Windows Credential Manager / Linux Secret Service. |
| **Python deps** | **`uv`** for the Python migration script's deps | 10–100× faster than pip. Single-file lockfile. |

### Why Tauri + Rust over Python + Flask (the sister-app stack)

The sister app (still.hobbies) runs Python + Flask + SQLite + PyWebView, and Renzo trusts it. We deliberately diverged here because:

1. **Renzo is making me code this**, so the Rust learning curve is mostly mine, not his. Frontend (Svelte) is plain enough that he can read and patch it.
2. **He wants to commercialize this later.** Tauri ships signed installers for Mac + Windows + Linux from one codebase, with a real auto-updater. Python wraps don't.
3. **libSQL's Rust client is mature; its Python client is pre-1.0.** Sync is the riskiest part of the system — better SDK matters.
4. **Smaller bundle, faster startup** — 5–10 MB vs ~50 MB, ~200 ms vs ~2 s. Matters when you ship to non-developers.
5. **The "Claude reads emails and writes to the DB" agent is a separate Python service** anyway. Tauri pushes us toward that clean separation instead of cramming it into the desktop process.

### Why libSQL/Turso over plain SQLite or Postgres

- **Plain SQLite**: would force us to write the cloud-sync layer ourselves (the single biggest source of bugs in the sister-app's bug ledger). Embedded replica eliminates that category of bug.
- **Postgres directly (Supabase, Neon)**: forces us to give up local-first behavior. Plant wifi flakes → entry blocked.
- **libSQL embedded replica**: local SQLite that auto-mirrors to a cloud DB. We get both.

---

## §4 — Big architectural decisions

### 4.1  Public repo, private data

The repo will be public on GitHub (Renzo wants to make money off it later — open-source app, commercial hosted offering). To make that safe:

| In the repo (public) | NOT in the repo (private) |
|---|---|
| All app source code | `.env` files |
| Schema migrations (SQL files) | `*.db`, `*.sqlite`, `*.sqlite3` |
| Type definitions / models | Data dumps, exports, CSVs |
| Lookup-table seed data (canonical grades, plants, etc.) | Production records |
| Docs, README, this brain | Backups |
| `.env.example` (with EMPTY values) | Auth tokens, OAuth secrets, Turso URLs |

Enforcement:
- `.gitignore` covers all of the above on day 1.
- Add a pre-commit hook (`gitleaks` or `trufflehog`) before the repo goes public.
- The README documents the "BYO database" setup: a new user clones the repo, pastes their own Turso URL/token into the keyring on first launch.
- Lookup seed data is structural (e.g. "C1, C2, C3, C4, RK1, FLEC are the valid CCC/FLEC values"), not transactional, so it's safe to commit.

### 4.2  Local-first with cloud mirror

- Local SQLite (libSQL) is the canonical store for the operator. All reads and writes go local first.
- `db.sync()` pushes local writes up and pulls remote changes down on a schedule (and on user-triggered "Sync Now").
- If wifi dies, the operator keeps working. Sync resumes when connectivity returns.
- The future management dashboard reads the **remote** Turso DB only — never the local file.
- The future Claude email agent writes to the **remote** Turso DB. The desktop app pulls those rows into a "to review" inbox.

### 4.3  Single canonical event log

Following the sister app's pattern: **one append-only `production_event` table is THE source of truth.** Every other table is either a lookup (categorical), a parameter (opening balances, settings), or computed on demand from `production_event`.

This means:
- The 5 PC WHSE sheets in Excel become **one Rust function** parameterized by warehouse_id and start_date that computes the ledger from `production_event` rows.
- The W6/W7 Summary sheets become **on-demand queries** (or a small materialized table if perf ever matters, which it won't at this scale).
- There is no `inventory_balance` table. Balances are always derived. Excel got this right; we just need to translate the formulas.

### 4.4  Canonicalize categoricals at write

Every free-text field that becomes a category goes through a `canonicalize_<field>` function in Rust before any insert/update:
- `GRADE` (3X50, 2X6, 4X8, 3.5, etc.)
- `PLANT` (W6, W7, W6/W7)
- `WHSE` (WHSE 1, WHSE 2, WHSE 5, WHSE 7, W3 — note: workbook uses both "WHSE 7" and "W7" inconsistently; canonicalize to one form)
- `SRC` (TNK 1, TNK 2, TNK 3, FLEC, W7, etc.)
- `SHIFT` (M, E, N)
- `WHSE SIDE` (LS, RS)
- `FLEC STAT` (DONE, plus whatever other states emerge)
- `DVO SIDE` (out of scope but the column exists)
- `CCC / FLEC` (C1, C2, C3, C4, RK1, RK2, RK3, RK4, FLEC)

Each of these gets:
1. A canonical enum in Rust (e.g. `enum Grade { G3x50, G2x6, G4x8, ... }`).
2. A `canonicalize_grade(raw: &str) -> Result<Grade, CanonError>` function.
3. A lookup table in the DB (`grade(id, code, display_name, sort_order)`) seeded from the canonical enum.
4. Foreign-key columns on `production_event` (e.g. `grade_id INTEGER NOT NULL REFERENCES grade(id)`).
5. A unit test per known input variant asserting it maps to the canonical form.

Why this matters: the current Excel workbook silently has both `WHSE 7` and `W7` in the same column. In Excel that's a cosmetic problem. In a relational DB joined against a `warehouse` lookup, it's a duplicate-row bug.

### 4.5  Operator-confirmed mutations for inventory side-effects

This is the single highest-priority real-money rule from the sister app's bug ledger. **External signals never auto-update inventory state.** When the future Claude email agent parses an email that says "1000kg moved from W6 to PC WHSE 1," the desktop app shows it as a pending event in a "Review" queue. Renzo clicks "Apply" only after physically verifying the receipt.

The pattern in code:
- New `production_event_pending` table for external-source rows.
- UI badge: "12 pending review."
- Apply button moves the row from `production_event_pending` → `production_event`.
- Reject button moves it to `production_event_rejected` with a reason.
- Never auto-promote.

### 4.6  Application-layer ledger query, not SQL view

The PC WHSE sheets compute a **per-(warehouse, grade, side) running balance**. In Rust, this is a function:

```rust
pub fn warehouse_ledger(
    conn: &libsql::Connection,
    warehouse_id: i64,
    start_date: NaiveDate,
) -> Result<Vec<LedgerRow>, Error> {
    // 1. Load opening balances from warehouse_opening_balance for this warehouse
    //    and any period <= start_date.
    // 2. Load production_event rows for this warehouse, prod_date >= start_date,
    //    sorted by prod_date.
    // 3. Walk the rows, accumulating per-(grade, side) running balance.
    // 4. Return the list with KG_IN, KG_OUT, FLEC_IN, FLEC_OUT, RUN_BAL filled.
}
```

Why a Rust function instead of a SQL view:
- Window functions + UNION with opening balance + per-(grade, side) partitioning is awkward in pure SQL but trivial in code.
- We can lock-in test it with a `tmp_db` fixture and 5 production rows.
- We can reuse it from the desktop UI, the management dashboard, and any future export path.

---

## §5 — Operational gotchas (must internalize from day 1)

These are the "you're going to regret this if you don't know it now" findings from the stack-verification research pass.

1. **Schema migrations apply to the REMOTE DB first.** libSQL embedded replicas pull schema down from the remote on sync. If you DDL only locally, the next sync overwrites your changes. The migration runner must connect to `TURSO_URL` directly and apply DDL there; the local replica picks it up automatically.

2. **First-launch needs a Turso DB to exist.** `Builder::new_remote_replica` against a non-existent remote DB fails. Two options for the bootstrap flow:
   - (A) Renzo creates the empty Turso DB out-of-band before first run. README documents this.
   - (B) The app detects "no remote DB exists" on first launch and creates one via the Turso Platform API (needs an API token). More ergonomic, more moving parts.
   - **Recommendation:** (A) for now, (B) later if it becomes annoying.

3. **Turso URL + token must be in OS keyring, NEVER baked into the binary.** A built binary with credentials in a compiled `.env` means anyone with the binary owns the DB. On first launch, prompt the user to paste their Turso URL + token; store via `tauri-plugin-stronghold` (or the `keyring` Rust crate). Read from keyring on every app start.

4. **Tailwind v4 Vite plugin order is load-bearing.** In `vite.config.ts`: SvelteKit plugin FIRST, then `@tailwindcss/vite`. If reversed, class scanning silently stops working. Copy from a known-working Tauri+SvelteKit+Tailwind v4 starter, don't write from scratch.

5. **SvelteKit SPA configuration.** Root `+layout.ts` must export:
   ```ts
   export const ssr = false;
   export const prerender = false;
   ```
   And `svelte.config.js` must use `@sveltejs/adapter-static` with `fallback: 'index.html'`. `tauri.conf.json` must point `frontendDist` at `build/`.

6. **App quit mid-sync.** Local writes are durable in SQLite even if `db.sync()` is interrupted. The push retries on next launch / next sync. UI should surface a sync-status indicator (last successful sync timestamp + a "Sync Now" button + an unread-changes count).

7. **macOS notarization timing.** Via App Store Connect API key (preferred over Apple ID password). First notarization 5–15 min, occasionally hours when Apple is slow. Wire up the signing pipeline in week 1, not week 12 — finding out it's broken at release time is painful.

8. **Windows code signing changed in 2023.** OV certs are HSM-only; EV certs need a USB HSM or a cloud signing service like **Azure Trusted Signing** (~$10/mo). You cannot just `signtool` a `.pfx` from CI anymore. Not blocking, but budget for it before commercializing.

9. **`tauri-specta` is still at 2.0.0-rc.24** as of May 2026. Pin the exact RC version in `Cargo.toml` and `package.json`. Don't bump on a whim. Plan B if it breaks at a Tauri minor release: switch to hand-written TypeScript interfaces (acceptable for ~20 commands) or `ts-rs`.

10. **Stay on Tauri ≥ 2.6** for an `tauri-plugin-updater` bug fix where NSIS updates failed when launched with command-line args containing spaces.

---

## §6 — What's in the source workbook

Source: `docs/reference/2025 CI PRODUCTION V2.xlsb`. 1,165 rows in the `Production` sheet (rows 2–6 are a legend, real data starts ~row 50). All findings below are from a survey pass; the **schema-extraction MD (next deliverable)** will document everything formally.

### 6.1  Sheets in scope (build against these)

| Sheet | What it is |
|---|---|
| `Production` | The append-only event log. 15 columns. Every charcoal batch logged here. **System of record.** |
| `PC WHSE 1` | Derived warehouse-1 ledger. Formula-driven below row 14. |
| `PC WHSE 2` | Derived warehouse-2 ledger. Same shape as WHSE 1. |
| `PC W3` | Derived warehouse-3 ledger. Same shape. |
| `PC WHSE 5` | Derived warehouse-5 ledger. Same shape. |
| `PC WHSE 7` | Derived warehouse-7 ledger. Has the richest data; use as the reference when reverse-engineering formulas. |
| `W6 Summary` | Plant-W6 production rollup. Pivot: date × tank × shift × grade rows; C1–C4 / RK1–RK4 / FLEC columns showing kg amounts. |
| `W7 Summary` | Plant-W7 production rollup. Same pivot shape. |

### 6.2  Sheets out of scope (do NOT build against)

| Sheet | Why out |
|---|---|
| `DVO IN`, `DVO OUT`, `PC W3 - DVO`, `PC WA7 - DVO` | Davao logistics ledger. Tracks Gothong Southern Shipping slips. Does not reference `Production` table. Future module. |

### 6.3  Production sheet structure

**Row 1 (headers):** `CCC RECV | PROD DATE | BATCH | SHIFT | GRADE | PLANT | WHSE | SRC | WT | CCC / FLEC | FLEC AMT | WHSE SIDE | FLEC STAT | DVO SIDE | UNIQUE TAG`

**Rows 2–6:** legend / valid-value reference (e.g. row 2 column SHIFT = "M", column CCC/FLEC = "C1"; row 3 SHIFT = "E", CCC/FLEC = "C2"). Treat as documentation, NOT data.

**Rows ~50 onward:** real data. Sample row 50:
```
CCC RECV=45996 (Excel date), BATCH=DECEMBER, GRADE=3X50,
PLANT=W6/W7, WHSE=WHSE 7, SRC=FLEC, WT=10686,
CCC/FLEC=C1, FLEC AMT=20, FLEC STAT=DONE,
UNIQUE TAG="45996--DECEMBER--3X50-W6 / W7-WHSE 7--FLEC-C1"
```

Note the empty segments (`--`) inside the UNIQUE TAG — fields that weren't filled in produce empty positions in the concatenation. The downstream WHSE sheets parse this tag by **fixed character offsets**, which is brittle. We will not replicate that. The backend uses the actual columns directly (`ccc_or_flec` field discriminates inflow vs outflow, not a substring of the tag).

### 6.4  PC WHSE sheet pattern (all 5 are structurally identical, only the warehouse selector in `C2` differs)

**Rows 1–13 (header / inputs):**
- `C1` = START date filter (e.g. 46091 = 2026-03-04 in Excel-date)
- `C2` = WHSE selector (e.g. "WHSE 7")
- Rows 4–12 = a per-grade × side starting-balance block. Columns: GRADE | RS | LS | STARTING-RS | STARTING-LS. The C/D values are computed (rolled forward); the E/F values are user-typed openings.

**Row 14 (ledger headers):** `UNIQUE TAG | RECV DATE | PROD DATE | SRC | GRADE | SIDE | TAG | STATE | KG IN | KG OUT | FLEC IN | FLEC OUT | RUN BAL`

**Row 15:** A single `UNIQUE(FILTER(PRODUCTION[UNIQUE TAG], ...))` formula that "spills" the matching tags down. Every column from B15 onward is `XLOOKUP(A15, PRODUCTION[UNIQUE TAG], PRODUCTION[<column>])`.

**Rows 16+:** Static values produced by the spill. NOT separate formulas. Important when reading via openpyxl — only row 15 has formulas, the rest are spilled values.

**The IN/OUT inference (a load-bearing business rule):** The WHSE sheets infer whether a Production row is an inflow or outflow by parsing the LAST hyphen-segment of the UNIQUE TAG. If it equals `"FLEC"` it's an inflow (KG IN, FLEC IN populated). Otherwise it's an outflow (KG OUT, FLEC OUT populated). **In the backend, do NOT replicate the substring trick. Use the `ccc_or_flec` column directly:** if the value is `FLEC`, it's an inflow; otherwise it's an outflow.

**The RUN BAL formula:** windowed `SUMIFS(KG_IN_range, grade, side) - SUMIFS(KG_OUT_range, ...)` plus an opening balance from the `B6#` spill. In the backend this is the Rust `warehouse_ledger` function (§4.6).

### 6.5  W6 / W7 Summary sheets

These are pivot views of `Production` filtered to a plant. Columns: GRADE | PLANT | DATE rows × C1 | C2 | C3 | C4 | RK1 | RK2 | RK3 | RK4 | FLEC | Grand Total. Daily detail on the right side, monthly rollup on the left. They're redundant with what a `SELECT ... GROUP BY plant, prod_date, ccc_or_flec` would produce. Don't persist them; compute on demand from `production_event`.

### 6.6  Observed categorical values (will become enum / lookup-table seeds)

From the legend rows + spot-checked real data:
- **SHIFT**: M, E, N (morning, evening, night — confirm with Renzo)
- **GRADE**: 3X50, 2X6, 3.5, 4X8 (and possibly more — enumerate during extraction)
- **PLANT**: W6, W7, W6/W7 (W6/W7 means a combined plant operation — confirm)
- **WHSE**: WHSE 1, WHSE 2, WHSE 5, WHSE 7, W3 (note: `WHSE 7` and `W7` both seen — needs canonicalization)
- **SRC**: TNK 1, TNK 2, TNK 3, FLEC, W7, plus likely more
- **CCC / FLEC**: C1, C2, C3, C4, RK1, RK2, RK3, RK4, FLEC
- **WHSE SIDE**: LS, RS (left side, right side)
- **FLEC STAT**: DONE (plus likely PENDING — confirm)
- **DVO SIDE**: out of scope; column exists, defer

---

## §7 — Proposed data model (preview, not the final spec)

This is a sketch. The full version with every column, type, FK, index, and check-constraint goes in the schema-extraction MD (next deliverable).

### 7.1  Core spine

```
production_event
  id                 INTEGER PRIMARY KEY AUTOINCREMENT  -- surrogate, NOT unique_tag
  ccc_recv_date      TEXT NOT NULL                       -- ISO date
  prod_date          TEXT NOT NULL
  batch              TEXT
  shift_id           INTEGER NOT NULL REFERENCES shift(id)
  grade_id           INTEGER NOT NULL REFERENCES grade(id)
  plant_id           INTEGER NOT NULL REFERENCES plant(id)
  warehouse_id       INTEGER NOT NULL REFERENCES warehouse(id)
  src_id             INTEGER NOT NULL REFERENCES source(id)
  weight_kg          REAL NOT NULL
  ccc_or_flec        TEXT NOT NULL CHECK (ccc_or_flec IN
                       ('C1','C2','C3','C4','RK1','RK2','RK3','RK4','FLEC'))
  flec_amount        REAL
  whse_side          TEXT NOT NULL CHECK (whse_side IN ('LS','RS'))
  flec_stat          TEXT       -- canonicalized at write
  dvo_side           TEXT       -- canonicalized at write (out-of-scope column, kept for parity)
  unique_tag         TEXT NOT NULL UNIQUE   -- computed at write from components
  notes              TEXT
  dirty              INTEGER NOT NULL DEFAULT 1   -- for any future push-to-X flow
  created_at         TEXT NOT NULL
  updated_at         TEXT NOT NULL

  INDEX (warehouse_id, prod_date)
  INDEX (grade_id, whse_side, prod_date)   -- for warehouse_ledger window
  INDEX (ccc_or_flec)                        -- for IN/OUT discrimination
```

### 7.2  Lookup tables (one per categorical)

```
grade           id PK, code (canonical), display_name, sort_order
plant           id PK, code, display_name
warehouse       id PK, code, display_name, branch
source          id PK, code, display_name
shift           id PK, code, display_name
```

All seeded from a `seed.sql` that ships in the repo. Adding a new value happens via migration, not via UI write (for now — could be UI later).

### 7.3  Opening balances (replaces the user-typed E6:F12 block on each WHSE sheet)

```
warehouse_opening_balance
  id                 INTEGER PRIMARY KEY AUTOINCREMENT
  warehouse_id       INTEGER NOT NULL REFERENCES warehouse(id)
  grade_id           INTEGER NOT NULL REFERENCES grade(id)
  side               TEXT NOT NULL CHECK (side IN ('LS','RS'))
  period_start_date  TEXT NOT NULL
  opening_kg         REAL NOT NULL DEFAULT 0
  opening_flec_count INTEGER NOT NULL DEFAULT 0
  UNIQUE (warehouse_id, grade_id, side, period_start_date)
```

### 7.4  Pending events (for the future Claude email agent)

```
production_event_pending  -- same columns as production_event plus:
  source              TEXT NOT NULL CHECK (source IN ('email','manual','import'))
  source_ref          TEXT       -- e.g. email message-id
  created_at          TEXT NOT NULL
  reviewed_at         TEXT
  reviewed_by         TEXT
  -- Apply moves to production_event; Reject moves to production_event_rejected
```

### 7.5  Support tables

```
schema_version        version INTEGER PRIMARY KEY, applied_at TEXT
app_settings          key TEXT PRIMARY KEY, value TEXT       -- defaults, period boundaries, etc.
drift_log             id PK, detected_at, kind, target_id, expected, actual, message
                       -- append-only; UI surfaces unresolved
```

### 7.6  The ledger function (not a table)

```rust
pub fn warehouse_ledger(
    conn: &libsql::Connection,
    warehouse_id: i64,
    start_date: NaiveDate,
) -> Result<Vec<LedgerRow>, Error>
```

Returns rows shaped like row 14+ of the PC WHSE sheets: `unique_tag, recv_date, prod_date, src, grade, side, tag, state, kg_in, kg_out, flec_in, flec_out, run_bal`. Seeded by `warehouse_opening_balance`.

---

## §8 — Reference architecture: 12 patterns to apply

These come from `docs/reference/architecture-reference.md`. **Apply them all** in this build. The reference doc has the long-form rationale; one-liners here as a checklist.

1. **Append-only migrations.** A `_MIGRATIONS` list of SQL strings, indexed by version. Never edit a previous migration; always append.
2. **Surrogate `id` + natural columns.** Never compose a primary key from free-text. `unique_tag` is a persisted/computed column for audit and export only.
3. **Canonicalize-at-write.** Every categorical free-text field has a `canonicalize_<field>` function applied at every write site. Lock-in tests for every variant.
4. **State machines as closed enums.** `FLEC STAT`, `DVO SIDE`, any future status flag = closed enum + `compute_*_transition(old, new)` helper. No scattered `if status == "foo"` conditionals.
5. **Dirty flag on mutable rows.** Cheap to add now, expensive to add later. Set 1 on every write; cleared to 0 only after the WHOLE downstream operation lands.
6. **Drift log table.** Append-only. Every silent-failure path writes here. UI surfaces with a "Resolve" action.
7. **Transaction context manager.** Every multi-statement write inside a `with_transaction { ... }` (or Rust equivalent). SQLite is fast enough that you should never not use a transaction for bulk writes.
8. **App settings k/v table.** UI-toggleable knobs, defaults, period boundaries.
9. **User-confirmed mutations for external-event side effects.** External signals never auto-update inventory state. Surface, confirm, then apply.
10. **Multi-line same-key is a recurring shape.** Code that groups by `(warehouse_id, grade_id, side)` MUST use a `Vec`/`HashMap<K, Vec<V>>`, never `HashMap<K, V>`. The latter silently collapses duplicates.
11. **Sync orderings are load-bearing.** If we add any push-to-X flow later: pull before push, always. Push always operates on post-pull state.
12. **Pre-launch checklist.** Before this app touches real data: idempotent migrations, canonicalize at every write, state-machine transitions tested, every multi-write in a transaction, drift log populated by every silent-failure path, lock-in tests for invariants, nightly DB backup.

### Anti-patterns (verbatim from the sister-app's bug ledger — don't repeat these)

1. Don't transition state flags based on partial completion.
2. Don't compose primary keys from raw free-text.
3. Don't use `{key: value}` dicts/maps for line-item-like data — use `[(k, list_of_v)]`.
4. Don't auto-apply external-event side effects to inventory.
5. Don't trust upstream data verbatim. Pair every ingestion with a canonicalize layer.
6. Don't skip telemetry on silent failures.
7. Don't optimize prematurely. No ORM, no caching layer until measured.

---

## §9 — Roadmap (order of operations)

Each step is a separate Claude Code session. Don't bundle.

### Step 1 — Schema extraction MD *(next session)*

Output: `/Users/renzosy/CI-ICTC-inventory-app/docs/schema-extraction.md`. ~10 pages of markdown. No code.

Per the spec in `docs/reference/charcoal-context-extraction-task.md` Part 2, sections 1–9. The doc must contain:
1. Executive summary.
2. The Production master table — column dictionary + enums.
3. The UNIQUE TAG composite key — exact spec, parsing rules, and why we drop it.
4. The WHSE ledger pattern — formula classes, IN/OUT inference, running balance, opening balance.
5. Plant summary sheets — aggregation grain.
6. Proposed relational schema — tables, columns, FKs, indexes.
7. Business rules catalog — `[ENFORCED IN SHEET]` / `[CONVENTION ONLY]` / `[INFERRED — CONFIRM WITH RENZO]` for each rule.
8. Open questions for Renzo.
9. Out-of-scope appendix (DVO, RC INVENTORY).

To do this properly: **convert the .xlsb → .xlsx via LibreOffice headless first**, then read formulas via openpyxl. `pyxlsb` only sees values, not formulas. Approximate command:
```bash
soffice --headless --convert-to xlsx --outdir docs/reference/ \
  "docs/reference/2025 CI PRODUCTION V2.xlsb"
```

### Step 2 — Repo scaffold

Output: a working Tauri v2 + Rust + SvelteKit + libSQL boot in `/Users/renzosy/CI-ICTC-inventory-app/`. Doesn't need to do anything functional yet — just needs to launch a window with a "Hello" page that successfully connects to a local libSQL file.

Includes:
- `src-tauri/` (Rust) with main.rs, db/mod.rs (connection + migrations runner that applies to remote first), commands/ (empty placeholder).
- `src/` (SvelteKit) with adapter-static config, Tailwind v4 + shadcn-svelte initialized, one route.
- `migrations/` with v1.sql (the schema from §7).
- `seed.sql` with the canonical lookup-table values.
- `.env.example`, `.gitignore`, README with BYO-Turso setup + keyring credential flow.
- `tauri.conf.json` configured for SPA mode.
- `vite.config.ts` with the correct plugin order.
- `Cargo.toml` and `package.json` with all deps pinned.

### Step 3 — First vertical slice

A single screen: "Log a production event." End-to-end through the whole stack.
- SvelteKit form with superforms + zod validation.
- `invoke('create_production_event', payload)` to a Rust command.
- Rust canonicalizes, validates, inserts into libSQL inside a transaction.
- libSQL syncs to Turso.
- Form returns success, list refreshes.

This is the smoke test for the whole stack. If this works, everything else is incremental.

### Step 4 — Excel data migration

Output: `scripts/migrate_from_xlsb.py`. One-shot Python script. Reads `docs/reference/2025 CI PRODUCTION V2.xlsb`, canonicalizes every row, inserts into the local libSQL DB. ~50–100 lines. Run once.

### Step 5 — Warehouse ledger view

Output: a Rust `warehouse_ledger` function + a SvelteKit route that calls it and displays the 13-column ledger table for a chosen warehouse + start_date. With lock-in tests asserting exact RUN BAL values against a fixture of 5 production rows.

### Step 6 — Iterate

Whichever screen hurts most in daily use comes next. Ask Renzo, don't guess.

### Later (defer until asked)

- Management dashboard URL — separate small app reading the same Turso DB. Probably SvelteKit + adapter-vercel + Supabase-style auth.
- Claude email-funnel agent — separate Python daemon. Anthropic SDK + Gmail API + `production_event_pending` writes.

---

## §10 — Reference files (in `docs/reference/`)

| File | What it is | When to open it |
|---|---|---|
| `charcoal-context-extraction-task.md` | Renzo's original brief for the extraction task. Has the full spec for the schema-extraction MD (Step 1). | Doing Step 1. |
| `architecture-reference.md` | The sister-app's distilled architectural patterns. Use the patterns, NOT the stack (we changed the stack). | Anytime you're applying a pattern (canonicalize, state machine, drift log, etc.). |
| `2025 CI PRODUCTION V2.xlsb` | The live production workbook for CI. **In scope.** | Doing Step 1 (extraction) and Step 4 (migration). |
| `CI RC INVENTORY.xlsb` | The RC inventory workbook. **Reference only — out of scope for this build.** | Don't open during the production-module build. Will be the source for a future RC-Inventory module. |

If you need to read the .xlsb files, install pyxlsb if missing:
```bash
pip3 install --break-system-packages pyxlsb pandas openpyxl
```

For formulas, convert .xlsb → .xlsx with LibreOffice headless first (pyxlsb only reads values).

---

## §11 — Open questions for Renzo

Mark each one with whether it's blocking a roadmap step. Don't surface a question to him until you actually need the answer.

| # | Question | Blocks |
|---|---|---|
| Q1 | Does `BATCH` (e.g. `"DECEMBER"`, `"MARCH"`) refer to the calendar month, or is it a batch-name convention that happens to align with months? Affects whether to store as TEXT or as a derived month-of(prod_date). | Schema extraction §6, schema design |
| Q2 | What does `CCC RECV` semantically mean (beyond being a date)? Is it the date the CCC raw material was received? Affects naming and what the column actually represents. | Schema extraction §2 |
| Q3 | What triggers a row to be `C1` vs `C2` vs `C3` vs `C4` vs `RK1-4` vs `FLEC`? Is this a quality grade, a process stage, a destination type, something else? | Schema extraction §7 (business rules), enum naming |
| Q4 | Are there validation rules we can't infer from data alone? E.g. "Grade 3X50 is only made at plant W6", "weight is always 5,000–25,000 kg", "FLEC AMT correlates with WT by a known ratio". | Schema extraction §7, CHECK constraints |
| Q5 | The workbook has both `WHSE 7` and `W7` in the WHSE column. What's the canonical form? | Canonicalize functions, lookup-table seed |
| Q6 | What does `FLEC STAT` mean and what values does it take besides `DONE`? Is it a workflow state machine? | State machine design (§4.4 / §8 pattern 4) |
| Q7 | Auth model for the management dashboard: who logs in, how many of them, do they need any write actions or strictly read? | Step "Management dashboard URL" (deferred) |
| Q8 | Do we want first-launch to auto-create the Turso DB via the Platform API, or is it OK to require the user to create it out-of-band? (See §5 gotcha #2.) | Bootstrap UX in Step 2 |

---

## §12 — Glossary (charcoal-domain terms)

Mostly inferred. Confirm with Renzo when relevant.

| Term | What it (probably) means |
|---|---|
| **CCC** | Charcoal something. Possibly "carbonized coconut coal" or a quality grade prefix. CCC RECV = receipt date of raw material. **Confirm.** |
| **FLEC** | A specific product type or process stage. Distinguished from C1–C4 (which appear to be quality grades). A row with `CCC/FLEC = FLEC` is treated as an inflow (material going INTO a warehouse) by the WHSE sheets. **Confirm.** |
| **GRADE** | Product size grade. Values: 3X50, 2X6, 4X8, 3.5 (and likely more). The numbers may refer to dimensions in inches or a sieve grade. **Confirm.** |
| **PLANT** | Production plant within CI. Values: W6, W7, W6/W7. The "W" likely stands for "warehouse-adjacent" or just "Wing". W6/W7 means a combined operation. **Confirm.** |
| **WHSE** | Warehouse where finished product is stored. Values: WHSE 1, WHSE 2, WHSE 5, WHSE 7, W3 (the workbook is inconsistent with prefixes). |
| **SRC** | Source — where the input material came from for this batch. Values: TNK 1, TNK 2, TNK 3 (storage tanks), FLEC, W7 (other plants/processes). |
| **SHIFT** | Production shift. M = Morning, E = Evening, N = Night. **Confirm.** |
| **WHSE SIDE** | Physical side of the warehouse where stock is placed. LS = Left Side, RS = Right Side. Used for running balances per (warehouse, grade, side). |
| **FLEC STAT** | Status of the FLEC operation for this row. Observed: DONE. **Confirm other values.** |
| **DVO SIDE** | Davao-side allocation flag. Out of scope for the CI Production module. |
| **RK1, RK2, RK3, RK4** | Subcategories of CCC/FLEC. Possibly "rework" levels, possibly different quality grades. **Confirm.** |
| **C1, C2, C3, C4** | CCC quality grades. Numerical ordering may matter (C1 best, C4 worst — or reverse). **Confirm.** |
| **RC** | Raw Coal. The upstream raw material. The `CI RC INVENTORY.xlsb` workbook tracks RC stock — out of scope for this build. |
| **DVO** | Davao. The sister branch ICTC's location. The DVO sheets in the production workbook track shipments from Cebu to Davao via Gothong Southern Shipping. Out of scope. |
| **UNIQUE TAG** | A 10-field hyphen-concatenated string Excel uses as a row identifier for the WHSE sheets to look up. Brittle (empty fields produce `--` runs, position-based parsing). The backend keeps it as a computed column for audit/export but does NOT use it as a primary key. |
| **TNK** | Tank. Holding tanks for processed material before warehouse storage. |

---

## §13 — Updating this brain

When something here turns out to be wrong, fix it in this file FIRST, then continue coding. The brain is the source of truth, not your conversation memory. A new session in 3 weeks should be able to pick up exactly where the last one left off by reading this top to bottom.

If you add a major architectural decision, give it its own subsection in §4. If you discover a new gotcha, add it to §5. If you finish a roadmap step, mark it complete in §9 and add a one-line "what was actually built" note. If a glossary term gets confirmed by Renzo, remove the "**Confirm.**" hedge.

Memory entries (in the auto-memory system at `~/.claude/projects/-Users-renzosy-CI-ICTC-inventory-app/memory/`) carry across sessions automatically — the brain is for project-level facts that need to live in the repo.
