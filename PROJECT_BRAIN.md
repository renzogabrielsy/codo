# codo — Project Brain

> Single source of truth for any new Claude Code session working in this repo.
> Last updated: **2026-05-07** (Step 1 corrections after Renzo walkthrough).
> If anything in here turns out to be wrong, fix it here first, then keep coding.

---

## §0 — Read this first

You (the new session) are picking up a project that has finished its planning phase. Nothing has been built yet. The next deliverable is the repo scaffold (Step 2). The schema is already worked out in `docs/schema-extraction.md`.

Before you do anything else, read in this order:
1. **This file** (`PROJECT_BRAIN.md`) — the consolidated context.
2. **`docs/schema-extraction.md`** — the formal schema spec that Step 2 builds against. Reflects Renzo's walkthrough corrections (post-survey-pass), not the raw inferences.
3. **`docs/reference/architecture-reference.md`** — the patterns from Renzo's sister app (still.hobbies). Don't copy the stack; do copy the patterns (canonicalize-at-write, drift log, dirty flag, state machines, etc.).
4. **`docs/reference/charcoal-context-extraction-task.md`** — the original brief Renzo wrote for the schema-extraction task. Useful background but partially superseded by the corrections in `docs/schema-extraction.md`.
5. **`docs/reference/2025 CI PRODUCTION V2.xlsb`** — the live workbook. Read with `pyxlsb` for values, convert to `.xlsx` via LibreOffice + read with `openpyxl` if you need formulas.
6. **`docs/reference/CI RC INVENTORY.xlsb`** — reference only. Future module, do not build against it yet.

The shortcut goes in your explanation, never in your work.

---

## §1 — What we're building (plain English)

Renzo runs **CI**, a prepared-charcoal manufacturing company in **Cebu, Philippines**. He oversees a sister branch, **ICTC**, in **Davao**. CI's plant has been logging production in a single Excel workbook (`2025 CI PRODUCTION V2.xlsb`) for years. We're replacing it with a local desktop app called **codo**.

### How charcoal actually moves through the business

This is the model the workbook implicitly encodes. codo makes it explicit.

1. **CI's plants distill or output charcoal.** There are two plants on the Cebu site:
   - **Plant W6** — has 4 tanks (`TNK 1`, `TNK 2`, `TNK 3`, `TNK 4`). Almost always used for grade `3X50`.
   - **Plant W7** — has 1 tank (logged as just `W7`). Also for `3X50` typically.
   - Other grades (`3.5`, `2X6`, `4X8`) are produced **directly as flecon bags** at the plant, bypassing tanks.

2. **A tank gets "assigned" to a (date, shift).** The foreman points at TNK 2 and says "this tank is May 31 morning." Everything that enters the tank during that shift is the production output of that shift assignment. Almost always one shift = one tank = one grade. The tank stays "assigned" until it's drawn down to empty. **Tank assignments are never logged explicitly** — they're a verbal/visual convention. codo reconstructs them from the rows that reference the tank.

3. **The tank gets drawn down two ways:**
   - **Partner company** ("CCC" in the legacy column names — just their codename) takes from the tank, weighs into carts, and reports back daily. They feed our charcoal into one of **their 4 crushers (`C1`–`C4`)** or **their 4 rotary kilns (`RK1`–`RK4`)**. Each report = one row in `Production` with `CCC/FLEC = C1..C4` or `RK1..RK4`.
   - **CI itself** drains the tank's remaining contents and bags it into **flecon bags** (one bag = one "flec"), storing them in a real warehouse. Each bagging event = one row in `Production` with `CCC/FLEC = FLEC`.

4. **Same-day batch transitions are a real thing.** CI batches by month. When a month rolls over (e.g. May → June), the foreman empties the current tank into FLEC to close the MAY batch, then starts the JUNE batch by feeding raw material into the same tank again. Two separate (tank, shift, day) cycles can land on the same calendar day, distinguished by the BATCH column.

5. **Flec bags live in physical warehouses.** WHSE 1, 2, 5, 7 hold CI Cebu's flec inventory, tracked per `(grade, side)` — each warehouse has a Left Side (`LS`) and Right Side (`RS`). Running balance is in **flec count, not kg.** WHSE 3 is the **Davao warehouse** (more on this below).

6. **Partner can also pull from flec inventory.** If they need more graded charcoal than the tanks have, they take bagged flec out of WHSE 1/2/5/7. That's logged the same way as a tank pull: `CCC/FLEC = C1..C4` or `RK1..RK4`, with `SRC = FLEC` to indicate "from already-bagged inventory" instead of `SRC = TNK n`.

### Why this is hard to track in Excel

- **Tank balance is unknown until empty.** CI never knows exactly how much was distilled into a tank — only the partner's per-day reports plus CI's final bag-up tell you the total in retrospect.
- **The same column carries two meanings.** `CCC / FLEC` mixes "what CI did" (FLEC = bagged) with "which partner equipment it went into" (C1–C4 = crushers, RK1–RK4 = kilns). codo splits this into a clean enum.
- **Warehouse-side inventory drifts** because the workbook's lookups don't canonicalize. `WHSE 7` and `W7` are the same warehouse but the spreadsheet treats them as different keys; 132 rows of `W7` data are silently invisible in the `PC WHSE 7` ledger today.
- **DVO (Davao) inflows are copy-pasted from emails** into a separate sheet, totally disconnected from the main Production log. Same warehouse (WHSE 3) but two different ledgers — one for inbound containers, one for partner takeouts.
- **No audit trail, no concurrent edit, no real query layer**, and you can't show management the numbers without a screenshot.

### What codo does

A local desktop app, single operator (Renzo), that:

1. Logs production events fast, with categorical fields canonicalized at write so `WHSE 7` and `W7` always end up as the same warehouse.
2. Keeps a local SQLite (libSQL) database that mirrors to a cloud Postgres-compatible DB (Turso) automatically.
3. Lets a future management dashboard read those numbers from a real URL with logins.
4. Tracks **two units of inventory in parallel**: flec count for WHSE 1/2/5/7, kg for WHSE 3 (Davao product is in PP sacks, dumped into carts on withdrawal, no sack count).
5. Tracks **DVO batches as first-class entities** with manual close/open lifecycle and live `transit_loss` + `yield_loss` displays.
6. Sets the stage for a **Claude-powered email-funnel agent** later — partner production reports emailed in get parsed and queued for Renzo to confirm with one click.
7. Is shipped as a public open-source repo, with no real data ever in the repo, so it can be commercialized later (sell hosted version, or sell deployment to other charcoal companies).

### Information density is a core principle

`codo` is replacing the source-of-truth spreadsheet. Every derived value the UI shows must be cross-checkable from the same screen — KPIs come with their input rows, balances come with their starting + ins − outs breakdown, forms preview their derived fields live. This is `feedback_show_solution.md` in the auto-memory and it applies to UI design AND backend code (prefer functions that return inputs alongside outputs). See §4.7.

---

## §2 — Scope for the FIRST build

**IN scope (build now):**
- The **Production module** for **CI Cebu**, including:
  - The main production event log (`production_event` — one row per workbook row except DVO inflows)
  - Lookup tables for grade, plant, warehouse, source location, shift, partner equipment
  - Five real warehouses (WHSE 1, 2, 3, 5, 7) with per-`(grade, side)` flec-count ledgers for 1/2/5/7
  - **DVO sub-system** for WHSE 3:
    - `dvo_receipt` table — per-container inflows from Davao (one row per Gothong slip)
    - `dvo_batch` table — first-class batches with manual open/close lifecycle, `transit_loss` and `yield_loss` displays
    - WHSE 3 ledger in **kg**, parameterized by `dvo_batch_id`
- Local SQLite database via libSQL embedded replica.
- Cloud sync to Turso (the management dashboard reads from there later).
- Excel migration script — one-shot Python — to bring in:
  - Production sheet rows → `production_event`
  - DVO IN sheet rows → `dvo_receipt`
  - PC WHSE * starting balance blocks → `warehouse_opening_balance`
  - The 1 confirmed-mistake duplicate `UNIQUE TAG` row → import one, log the other to `drift_log`
- Warehouse ledger views (per warehouse, per batch for WHSE 3).

**OUT of scope (defer, do not build):**
- **RC Inventory module** — the `CI RC INVENTORY.xlsb` workbook. Separate raw-coal inventory tracking, future module. Keep readable as reference.
- **ICTC Davao plant operations** — different management. Only ICTC's *outbound containers to Cebu* are in scope (via DVO IN); their internal ops are not.
- **`DVO OUT`, `PC W3 - DVO`, `PC WA7 - DVO` sheets** — DVO IN is in; the others are legacy/parallel logistics ledgers Renzo gave up maintaining. Do not import or replicate. The legacy `AVG LOSS` calculation lives only in `PC W3 - DVO` and is replaced in codo by per-batch `transit_loss` + `yield_loss` displays.
- **Management dashboard URL** — the *capability* to expose data is part of the architecture from day one (that's why we sync to Turso), but the actual dashboard is a separate small app built later.
- **Multi-tenant features** — single-operator app. Multi-tenant comes if/when Renzo sells to a second company.
- **Auth for the desktop app** — single operator on a single Mac. No login screen.

---

## §3 — Tech stack (verified May 2026)

This stack was verified by a focused web-research pass against the latest releases as of 2026-05-07. Three changes from the initial proposal based on that research; reasons inline.

| Layer | Pick | Notes |
|---|---|---|
| **Shell** | **Tauri v2** (≥ 2.6) | Native window. ~5–10 MB binaries. Multi-platform. Real signed auto-updater. |
| **Backend lang** | **Rust** (current stable) | Runs inside Tauri. Talks to libSQL. Renzo isn't expected to write Rust — just to read it. |
| **DB client** | **`libsql` Rust crate** | Turso's official client. Mature. Embedded-replica support is first-class. |
| **Local DB → cloud sync** | **libSQL embedded replica → Turso cloud** | Local SQLite file IS the replica. Auto-syncs to Turso with `db.sync()`. **Caveat:** Turso has marked embedded replicas as "legacy" in favor of "Turso Sync" (still in beta as of May 2026). Plan: ship on embedded replicas now, migrate to Turso Sync on GA. SQL surface is compatible. |
| **Frontend framework** | **SvelteKit 2 + Svelte 5 (runes)**, in **SPA mode** via `@sveltejs/adapter-static` | No SSR (we're inside a webview). Root `+layout.ts` must export `ssr=false`, `prerender=false`. `svelte.config.js` uses `fallback: 'index.html'`. |
| **Styling** | **Tailwind CSS v4** (via `@tailwindcss/vite`) | **Vite plugin order matters:** SvelteKit plugin first, Tailwind second, or class scanning silently breaks. Use `@reference "tailwindcss";` at the top of any `<style>` block that uses `@apply`. |
| **UI components** | **shadcn-svelte** (Tailwind v4 + Svelte 5 fully supported) | Copy-paste components, you own the code. Looks like a real app. |
| **Charts** | **Chart.js + thin Svelte 5 wrapper** *(changed from LayerChart)* | LayerChart 2's Svelte 5 line is on `2.0.0-next.62` prerelease. Chart.js is bulletproof. |
| **Type bridge (Rust ↔ TS)** | **`tauri-specta`** *(pinned to exact RC version `2.0.0-rc.24`)* | Generates TS types from Rust structs and `#[tauri::command]` handlers. Pin in both `Cargo.toml` and `package.json`. Plan B if it breaks: hand-write TS interfaces or switch to `ts-rs`. |
| **Forms** | **`sveltekit-superforms` + `zod`** | Works with Svelte 5 runes; expect `$derived` wrappers around form state in some places. |
| **State** | **Svelte 5 runes** + native stores | No Redux, no Zustand. |
| **Excel migration script** | **Standalone Python script** in `/scripts/` | Uses `openpyxl` for `.xlsx` (after converting `.xlsb` via LibreOffice headless) or `pyxlsb` for value-only reads. One-shot. Not part of the binary. |
| **Claude email-funnel agent (later)** | **Separate Python daemon** | Lives outside the desktop app entirely. Talks to the same Turso DB via the libSQL HTTP API. Uses the Anthropic Python SDK. Decoupling is a feature. |
| **Build / packaging** | **Tauri's built-in bundler** | Outputs `.dmg` (macOS), `.msi` (Windows), `.AppImage` (Linux). Auto-update via `tauri-plugin-updater` v2. |
| **Secrets at runtime** | **OS keyring** via `tauri-plugin-stronghold` or the `keyring` Rust crate | **Critical:** Turso URL + token CANNOT live in a baked-in `.env`. Prompt on first launch, store in macOS Keychain / Windows Credential Manager / Linux Secret Service. |
| **Python deps (migration script)** | **`uv`** | 10–100× faster than pip. Single-file lockfile. |

### Why Tauri + Rust over Python + Flask (the sister-app stack)

The sister app (still.hobbies) runs Python + Flask + SQLite + PyWebView. We deliberately diverged because:

1. **Renzo is making me code this** — Rust learning curve is mostly mine, not his. Frontend (Svelte) is plain enough that he can read and patch it.
2. **He wants to commercialize this later.** Tauri ships signed installers for Mac + Windows + Linux from one codebase, with a real auto-updater. Python wraps don't.
3. **libSQL's Rust client is mature; its Python client is pre-1.0.** Sync is the riskiest part of the system — better SDK matters.
4. **Smaller bundle, faster startup** — 5–10 MB vs ~50 MB, ~200 ms vs ~2 s. Matters for non-developers.
5. **The future Claude email-funnel agent is a separate Python service** anyway. Tauri pushes us toward that clean separation.

### Why libSQL/Turso over plain SQLite or Postgres

- **Plain SQLite** would force us to write the cloud-sync layer ourselves (the single biggest source of bugs in the sister-app's bug ledger). Embedded replica eliminates that category.
- **Postgres directly (Supabase, Neon)** forces us to give up local-first behavior. Plant wifi flakes → entry blocked.
- **libSQL embedded replica** — local SQLite that auto-mirrors to a cloud DB. We get both.

---

## §4 — Big architectural decisions

### 4.1  Public repo, private data

The repo is public on GitHub at [github.com/renzogabrielsy/codo](https://github.com/renzogabrielsy/codo). Renzo wants to commercialize later (open-source app, hosted offering). To make this safe:

| In the repo (public) | NOT in the repo (private) |
|---|---|
| All app source code | `.env` files |
| Schema migrations (SQL files) | `*.db`, `*.sqlite`, `*.sqlite3` |
| Type definitions / models | Data dumps, exports, CSVs |
| Lookup-table seed data (canonical grades, plants, etc.) | Production records |
| Docs, README, this brain | Backups, `.xlsb`, `.xlsx` |
| `.env.example` (with EMPTY values) | Auth tokens, OAuth secrets, Turso URLs |

Enforcement:
- `.gitignore` covers all of the above on day 1 (already in place).
- Add a pre-commit hook (`gitleaks` or `trufflehog`) before any major release.
- README documents the "BYO database" setup: a new user clones, pastes their own Turso URL/token into the keyring on first launch.
- Lookup seed data is structural (e.g. valid CCC/FLEC values), not transactional.

### 4.2  Local-first with cloud mirror

- Local SQLite (libSQL) is the canonical store for the operator. All reads and writes go local first.
- `db.sync()` pushes local writes up and pulls remote changes down on a schedule (and on user-triggered "Sync Now").
- If wifi dies, the operator keeps working. Sync resumes when connectivity returns.
- The future management dashboard reads the **remote** Turso DB only — never the local file.
- The future Claude email agent writes to the **remote** Turso DB. The desktop app pulls those rows into a "to review" inbox.

### 4.3  Single canonical event log + per-subsystem auxiliary ledgers

- **`production_event`** is the source of truth for everything CI logs: bagging events, partner crusher feeds, partner kiln feeds, DVO outflows from WHSE 3.
- **`dvo_receipt`** is its own table because DVO inflows have unique fields (Gothong slip, declared vs actual weight, sack count, container-yard pullout status) that don't fit on a `production_event` row.
- **`dvo_batch`** is its own table because batches are first-class entities the operator opens, closes, and reads loss numbers off of.
- **The 5 PC WHSE sheets** become **two Rust functions**: `warehouse_ledger_flec(warehouse_id, start_date)` for WHSE 1/2/5/7 (flec-count units) and `dvo_batch_ledger(dvo_batch_id)` for WHSE 3 (kg units, per batch).
- The W6/W7 Summary sheets become on-demand `GROUP BY` queries — never persisted.

### 4.4  Canonicalize categoricals at write

Every free-text field that becomes a category goes through a `canonicalize_<field>` function in Rust before any insert/update:
- `GRADE` (3X50, 2X6, 3.5, 4X8)
- `PLANT` (W6, W7, W6/W7, DVO)
- `WHSE` — the *destination* warehouse for the event. Canonical: `WHSE 1`, `WHSE 2`, `WHSE 3`, `WHSE 5`, `WHSE 7`. The workbook's `W6`/`W7` values in this column are cosmetic noise from before auto-fill (Renzo confirmed: "logically should be blank"); migration treats them as NULL.
- `SRC` — the *source location* of the product. Tanks (`TNK 1..4`), the W7 plant tank (logged as `W7`), direct plant output (`W6`), already-bagged inventory (`FLEC`), and Davao containers (`DVO`).
- `SHIFT` (M, E, N — only M observed in real data)
- `WHSE SIDE` (LS, RS — for WHSE 1/2/5/7 only; for WHSE 3 the side info comes from the linked `dvo_batch.side`)
- `FLEC STAT` (DONE plus pending — the full set is still Q for Renzo)
- `CCC / FLEC` → renamed `disposition` in the schema. Closed enum: `FLEC` (CI bagged into a warehouse), `C1..C4` (partner crusher feed), `RK1..RK4` (partner kiln feed).

Each of these gets:
1. A canonical enum in Rust.
2. A `canonicalize_<field>(raw: &str) -> Result<<Field>, CanonError>` function.
3. A lookup table in the DB seeded from the canonical enum.
4. Foreign-key columns on `production_event`.
5. A unit test per known input variant asserting it maps to the canonical form.

### 4.5  Operator-confirmed mutations for inventory side-effects

Single highest-priority real-money rule from the sister app's bug ledger. **External signals never auto-update inventory state.** When the future Claude email agent parses an email that says "1000kg moved from W6 to PC WHSE 1," the desktop app shows it as a pending event in a "Review" queue. Renzo clicks "Apply" only after physically verifying.

The pattern in code:
- New `production_event_pending` table for external-source rows (deferred until the email agent ships — don't add the table prematurely).
- UI badge: "12 pending review."
- Apply button moves the row from `production_event_pending` → `production_event`.
- Reject button moves it to `production_event_rejected` with a reason.
- Never auto-promote.

### 4.6  Application-layer ledger queries, not SQL views

The PC WHSE sheets compute running balances. In Rust, these are functions:

```rust
pub fn warehouse_ledger_flec(
    conn: &libsql::Connection,
    warehouse_id: i64,
    start_date: NaiveDate,
) -> Result<Vec<FlecLedgerRow>, Error> {
    // Returns one LedgerRow per production_event matching this warehouse,
    // sorted by recv_date, with computed kg_in/kg_out/flec_in/flec_out/run_bal_flec.
    // Seeded by warehouse_opening_balance.
}

pub fn dvo_batch_ledger(
    conn: &libsql::Connection,
    dvo_batch_id: i64,
) -> Result<DvoBatchLedger, Error> {
    // Returns: dvo_receipts (inflows, kg) +
    //          production_events with dvo_batch_id (outflows, kg),
    //          interleaved by date, with running kg balance.
    // Plus: transit_loss and yield_loss summary numbers.
}
```

Why functions instead of SQL views:
- Window functions + UNION with opening balance + per-key partitioning is awkward in pure SQL but trivial in code.
- Lock-in testable with a `tmp_db` fixture and 5 production rows + 1 opening balance row.
- Reusable from the desktop UI, the future management dashboard, and any future export path.

### 4.7  Always show the solution (information density)

codo is replacing the source-of-truth spreadsheet. Every computed number the UI shows must be cross-checkable from the same screen:

- **Ledger views**: every running balance row shows `starting + ins − outs = current`. Don't display the result alone.
- **KPIs / aggregates**: show the contributing row count and the period, with drill-down into source rows.
- **Forms**: derived fields (`unique_tag` preview, IN/OUT direction, current opening balance) update live as fields fill in.
- **Reports**: every total is accompanied by the breakdown that produces it.
- **DVO batch view**: shows live `transit_loss = (sum_dvo_declared − sum_cebu_declared) / sum_dvo_declared`, live `yield_loss = (sum_cebu_declared − sum_partner_takes) / sum_cebu_declared`, with the underlying receipt and outflow rows visible right there.

Backend-side: prefer functions that return inputs alongside outputs. The `warehouse_ledger_flec` function above returns every input column AND the running balance — not the balance alone.

This is `feedback_show_solution.md` in the auto-memory. Default dense in development; we can simplify later if it overwhelms, but always have the data ready to surface.

---

## §5 — Operational gotchas (must internalize from day 1)

These are the "you're going to regret this if you don't know it now" findings from the stack-verification research pass.

1. **Schema migrations apply to the REMOTE DB first.** libSQL embedded replicas pull schema down from the remote on sync. If you DDL only locally, the next sync overwrites your changes. The migration runner must connect to `TURSO_URL` directly and apply DDL there; the local replica picks it up automatically.

2. **First-launch needs a Turso DB to exist.** `Builder::new_remote_replica` against a non-existent remote DB fails. Two options:
   - (A) Renzo creates the empty Turso DB out-of-band before first run. README documents this.
   - (B) The app detects "no remote DB exists" on first launch and creates one via the Turso Platform API (needs an API token).
   - **Recommendation:** (A) for now, (B) later if it becomes annoying.

3. **Turso URL + token must be in OS keyring, NEVER baked into the binary.** A compiled binary with credentials in a baked `.env` means anyone with the binary owns the DB. On first launch, prompt for Turso URL + token; store via `tauri-plugin-stronghold` (or `keyring` Rust crate). Read from keyring on every app start.

4. **Tailwind v4 Vite plugin order is load-bearing.** In `vite.config.ts`: SvelteKit plugin FIRST, then `@tailwindcss/vite`. If reversed, class scanning silently stops working. Copy from a known-working Tauri+SvelteKit+Tailwind v4 starter.

5. **SvelteKit SPA configuration.** Root `+layout.ts` must export:
   ```ts
   export const ssr = false;
   export const prerender = false;
   ```
   `svelte.config.js` must use `@sveltejs/adapter-static` with `fallback: 'index.html'`. `tauri.conf.json` points `frontendDist` at `build/`.

6. **App quit mid-sync.** Local writes are durable in SQLite even if `db.sync()` is interrupted. The push retries on next launch / next sync. UI surfaces a sync-status indicator (last successful sync timestamp + "Sync Now" button + unread-changes count).

7. **macOS notarization timing.** Via App Store Connect API key. First notarization 5–15 min, sometimes hours. Wire the signing pipeline in week 1, not week 12.

8. **Windows code signing changed in 2023.** OV certs are HSM-only; EV certs need a USB HSM or cloud signing service like Azure Trusted Signing (~$10/mo). Not blocking, but budget for it before commercializing.

9. **`tauri-specta` is still at 2.0.0-rc.24** as of May 2026. Pin the exact RC. Don't bump on a whim. Plan B if it breaks: hand-written TS interfaces (acceptable for ~20 commands) or `ts-rs`.

10. **Stay on Tauri ≥ 2.6** for an `tauri-plugin-updater` bug fix where NSIS updates failed when launched with command-line args containing spaces.

11. **Workbook copy in `docs/reference/` is a snapshot, not live.** The live file is at `/Users/renzosy/Documents/1A WORK FILES/PRODUCTION/2025 CI PRODUCTION V2.xlsb`. Renzo confirmed the in-repo copy is sufficient for nailing schema, but if a Step 4 migration run needs current data, refresh first.

12. **First-launch flow uses the Turso Platform API to auto-create the database.** Renzo chose Path B in Q10 because codo is meant to be a shippable product — minimal friction matters. The flow:
    - First launch shows a one-screen onboarding asking for **one Turso Platform API token** (Renzo creates this once at Turso's web UI).
    - codo calls the Platform API to create a new DB (and group/org if needed), receiving back the DB URL + a DB-level auth token.
    - Both the URL and the DB-level token are stored in OS keyring via `tauri-plugin-stronghold`. The Platform API token is **not retained** — codo throws it away after creation, so even if the binary is leaked the attacker can't create more DBs on Renzo's account.
    - From then on, codo reads URL + token from keyring at boot. No prompts.
    - Edge cases the onboarding handles: Platform API token invalid (re-prompt with helpful error), DB name collision (auto-suffix), org/group selection (prompt if multiple available, default if one), token expiry (prompt for new Platform API token only when needed).
    - Step 2 includes a `turso_platform.rs` module wrapping the relevant Platform API endpoints (`POST /v1/organizations/{org}/databases`, `POST /v1/organizations/{org}/databases/{name}/auth/tokens`).

---

## §6 — What's in the source workbook

Source: `docs/reference/2025 CI PRODUCTION V2.xlsb`. Survey done with `pyxlsb` (values only — no formula text). 769 non-empty production rows in the `Production` sheet (data starts ~row 12, runs through ~row 1165). For complete column-by-column treatment, see `docs/schema-extraction.md`.

### 6.1  Sheets in scope

| Sheet | What it is |
|---|---|
| `Production` | The append-only event log. 15 columns + trailing blank. Every CI bagging, every partner crusher feed, every partner kiln feed, every DVO outflow. **System of record.** |
| `DVO IN` | Per-container inflows from Davao. Renzo copy-pastes from emails. Gothong tracking slips, declared vs actual weight, sack count, batch/side assignment, "for pullout at Cebu CY" status. **In scope** — codo absorbs this into a `dvo_receipt` table. |
| `PC WHSE 1` | Derived warehouse-1 ledger. Formula-driven below row 14. Flec count units. |
| `PC WHSE 2` | Same shape as WHSE 1. |
| `PC WHSE 5` | Same shape. |
| `PC WHSE 7` | Same shape. **Has the richest data — use as reference when reverse-engineering ledger logic.** |
| `W6 Summary` | Plant-W6 production rollup. Pivot view of `Production` filtered to plant W6. |
| `W7 Summary` | Plant-W7 production rollup. Same pivot shape. |

### 6.2  Sheets out of scope

| Sheet | Why out |
|---|---|
| `PC W3` | Currently broken in the workbook — selector says `W3` but Production rows use `WHSE 3`. Also, WHSE 3 needs a different ledger (kg, per-batch) that the legacy template doesn't model. codo replaces with `dvo_batch_ledger`. |
| `DVO OUT` | Legacy logistics ledger Renzo gave up maintaining. Out. |
| `PC W3 - DVO` | Legacy per-batch DVO tracker, "month behind because tedious." codo replaces with the live `dvo_batch_ledger` (kg balance + transit_loss + yield_loss). Out for migration. |
| `PC WA7 - DVO` | Same legacy tracker, even older data. Out. |

### 6.3  Production sheet headers (16 columns; col 15 always blank)

`CCC RECV | PROD DATE | BATCH | SHIFT | GRADE | PLANT | WHSE | SRC | WT | CCC / FLEC | FLEC AMT | WHSE SIDE | FLEC STAT | DVO SIDE | UNIQUE TAG | (blank)`

In codo's schema, columns are renamed to remove legacy partner-codename baggage:
- `CCC RECV` → `recv_date` (date the row was logged — partner reports OR CI's draw-down events; semantics confirmed by Renzo)
- `CCC / FLEC` → `disposition` (closed enum: `FLEC | C1..C4 | RK1..RK4`)
- `WHSE` (cosmetic blanks normalized) → `warehouse_id` (FK; nullable for tank-stage events where only `SRC` is meaningful)
- `WHSE SIDE` (when value is `LS`/`RS`) → `whse_side`
- `WHSE SIDE` (when value is a DVO batch code like `NOVEMBER2025RIGHT`) → parsed into `dvo_batch_id` FK; `whse_side` itself goes NULL
- `DVO SIDE` — always blank in observed data; column reserved, not modeled in v1

Rows 2–6 are a **legend** (sample valid values, not data) — see `docs/schema-extraction.md` §2 for the table.

### 6.4  PC WHSE sheet pattern (1, 2, 5, 7 — structurally identical)

- **Rows 1–13:** header block with `START:` date filter, `WHSE:` selector, FLECON computed balances (rolled forward), and STARTING user-typed openings per `(grade, side)`.
- **Row 14:** ledger headers: `UNIQUE TAG | RECV DATE | PROD DATE | SRC | GRADE | SIDE | TAG | STATE | KG IN | KG OUT | FLEC IN | FLEC OUT | RUN BAL`.
- **Row 15:** spill anchor — single `UNIQUE(FILTER(PRODUCTION[UNIQUE TAG], WHSE = $C$2, recv_date >= $C$1))` formula. Every column from B15 onward is `XLOOKUP(A15#, ..., PRODUCTION[<col>])`. Static values in rows 16+.
- **STATE column** infers IN/OUT from the LAST hyphen-segment of `UNIQUE TAG`: `FLEC` → IN, anything else → OUT. **codo uses `disposition` directly instead of the substring trick.**
- **RUN BAL** is per-`(grade, side)` running balance in **flec count units, NOT kilograms.** Excel does not run a kg balance.

### 6.5  W6 / W7 Summary sheets

Pivot views of `Production` filtered to one plant. Aggregation grain: monthly rollup on the left side (`PLANT × BATCH`), daily detail on the right side (`PROD DATE × BATCH × SRC × SHIFT × GRADE`). Columns C1–C4, RK1–RK4, FLEC are kg sums per disposition. **Redundant** with `SELECT … GROUP BY` queries from `production_event`. codo computes on demand, doesn't persist.

### 6.6  DVO IN sheet (the one we absorb)

Header columns include: `GOTHONG TRACKING SLIPS`, `ACTUAL WEIGHT`, `DIFF / LOSS`, `DATE RECV`, `WHSE`, `BATCH / SIDE`, `SKS` (sacks), plus IN/OUT count summaries and "for pullout at Cebu CY" status indicators. Each row is a container arrival. Renzo copy-pastes from emails. codo's `dvo_receipt` table mirrors these fields with renames for clarity (Davao declared weight vs Cebu declared weight as the two scales of truth).

### 6.7  Observed categorical values (will become enum / lookup-table seeds)

- **SHIFT**: `M` (legend lists `E`, `N` but only `M` observed in the 769 data rows)
- **GRADE**: `3X50`, `2X6`, `3.5`, `4X8` (legend; `4X8` not observed but reserved)
- **PLANT**: `W6`, `W7`, `W6/W7` (combined operation), `DVO`
- **WHSE**: `WHSE 1`, `WHSE 2`, `WHSE 3`, `WHSE 5`, `WHSE 7` (canonical). `W6` and `W7` cosmetic in this column → migrate as NULL.
- **SRC**: `TNK 1`, `TNK 2`, `TNK 3`, `TNK 4` (W6 plant tanks), `W7` (W7 plant tank), `W6` (W6 direct), `FLEC` (already-bagged inventory), `DVO` (Davao containers)
- **CCC / FLEC** (`disposition`): `C1`, `C2`, `C3`, `C4` (partner crushers), `RK1`, `RK2`, `RK3`, `RK4` (partner kilns), `FLEC` (CI bagging)
- **WHSE SIDE**: `LS`, `RS` (only valid for WHSE 1/2/5/7); on WHSE 3 rows the column carries DVO batch codes like `NOVEMBER2025RIGHT` which migrate into `dvo_batch_id`.
- **FLEC STAT**: `DONE` only. **Legacy column.** Renzo confirmed it used to track whether he had manually copied the row into the WHSE sheets. Now that the WHSE sheets auto-fill, the column is dead — conditional formatting referencing it still exists in the workbook but it's cosmetic. codo imports the column as a nullable TEXT for historical fidelity but does NOT validate, transition, or write to it. If we ever revive a "review pending" workflow, that's a real state machine in a future migration, not this column.

---

## §7 — Proposed data model (preview)

This is the sketch. Full version with every column, type, FK, index, and CHECK constraint lives in `docs/schema-extraction.md` §6. Step 2 implements it as `migrations/v1.sql`.

### 7.1  Lookup tables

```
shift               id PK, code (M|E|N), display_name, sort_order
grade               id PK, code (3X50|2X6|3.5|4X8), display_name, sort_order
plant               id PK, code (W6|W7|W6/W7|DVO), display_name, branch (CI|ICTC)
warehouse           id PK, code (WHSE 1|...|WHSE 7), display_name, branch, default_unit (flec_count|kg)
source_location     id PK, code (TNK 1..4|W7|W6|FLEC|DVO), display_name, kind (tank|plant_direct|warehouse_flec|dvo_container)
partner_equipment   id PK, code (C1..C4|RK1..RK4), display_name, kind (crusher|kiln)
```

Seeded from `seed.sql` shipped in the repo.

### 7.2  Core spine — `production_event`

```
production_event
  id                 INTEGER PK AUTOINCREMENT      -- surrogate, NOT unique_tag
  recv_date          TEXT NOT NULL                  -- ISO date (was: CCC RECV)
  prod_date          TEXT                           -- ISO date; nullable (DVO outflow rows often omit)
  batch              TEXT NOT NULL                  -- 'NOVEMBER'..'DECEMBER'; can differ from month-of(prod_date) at month boundaries
  shift_id           INTEGER REFERENCES shift(id)   -- nullable
  grade_id           INTEGER NOT NULL REFERENCES grade(id)
  plant_id           INTEGER NOT NULL REFERENCES plant(id)
  warehouse_id       INTEGER REFERENCES warehouse(id)        -- nullable (tank-stage events have NULL)
  source_location_id INTEGER NOT NULL REFERENCES source_location(id)  -- the SRC; truthful field
  weight_kg          REAL NOT NULL CHECK (weight_kg > 0)

  disposition_kind   TEXT NOT NULL CHECK (disposition_kind IN ('flec_bagging','partner_crusher','partner_kiln'))
  partner_equipment_id INTEGER REFERENCES partner_equipment(id)  -- NOT NULL when disposition_kind != 'flec_bagging'
  flec_count         INTEGER CHECK (flec_count IS NULL OR flec_count > 0)  -- bag count when relevant

  whse_side          TEXT CHECK (whse_side IS NULL OR whse_side IN ('LS','RS'))
  flec_stat          TEXT
  dvo_batch_id       INTEGER REFERENCES dvo_batch(id)         -- nullable; non-NULL on DVO outflows from WHSE 3

  unique_tag         TEXT NOT NULL UNIQUE                     -- computed at write from canonical components
  notes              TEXT
  dirty              INTEGER NOT NULL DEFAULT 1
  created_at         TEXT NOT NULL
  updated_at         TEXT NOT NULL

  INDEX (warehouse_id, recv_date)
  INDEX (grade_id, whse_side, recv_date)
  INDEX (disposition_kind)
  INDEX (plant_id, prod_date)
  INDEX (dvo_batch_id)
```

### 7.3  Opening balances — `warehouse_opening_balance`

Replaces the `STARTING` block (E6:F12) on each WHSE sheet. Operator can write a new opening balance row any time — UI presents it as "as of today, WHSE 7 RS for 3X50 has 53 flec on hand" without exposing the period-start concept. The ledger function uses the most recent opening balance dated on or before the START date.

```
warehouse_opening_balance
  id                  INTEGER PK
  warehouse_id        INTEGER NOT NULL REFERENCES warehouse(id)
  grade_id            INTEGER NOT NULL REFERENCES grade(id)
  side                TEXT NOT NULL CHECK (side IN ('LS','RS'))
  period_start_date   TEXT NOT NULL
  opening_flec_count  INTEGER NOT NULL DEFAULT 0
  notes               TEXT
  created_at          TEXT NOT NULL
  updated_at          TEXT NOT NULL
  UNIQUE (warehouse_id, grade_id, side, period_start_date)
```

WHSE 3 doesn't use this table — it uses per-batch ledgers via `dvo_batch` (each batch is its own self-contained period seeded by the receipts).

### 7.4  DVO sub-system

```
dvo_batch
  id              INTEGER PK
  code            TEXT UNIQUE NOT NULL          -- 'NOVEMBER2025RIGHT'
  warehouse_id    INTEGER NOT NULL REFERENCES warehouse(id)  -- WHSE 3 today
  start_month     INTEGER NOT NULL CHECK (1..12)
  year            INTEGER NOT NULL
  side            TEXT NOT NULL CHECK (side IN ('LEFT','RIGHT'))
  status          TEXT NOT NULL CHECK (status IN ('open','closed'))
  opened_at       TEXT NOT NULL
  closed_at       TEXT          -- NULL until manual close
  closed_by       TEXT
  notes           TEXT
  -- Frozen-on-close metrics, shown in UI:
  frozen_transit_loss   REAL    -- (sum_dvo_declared - sum_cebu_declared) / sum_dvo_declared at close
  frozen_yield_loss     REAL    -- (sum_cebu_declared - sum_partner_takes) / sum_cebu_declared at close

dvo_receipt
  id                       INTEGER PK
  dvo_batch_id             INTEGER NOT NULL REFERENCES dvo_batch(id)
  recv_date                TEXT NOT NULL
  gothong_slip             TEXT
  sack_count               INTEGER
  dvo_declared_weight_kg   REAL NOT NULL CHECK (> 0)   -- Davao's BOL number
  cebu_declared_weight_kg  REAL NOT NULL CHECK (> 0)   -- Cebu scale at receipt; truth
  at_cebu_cy               INTEGER NOT NULL DEFAULT 0  -- bool: still at Cebu container yard, not yet unloaded
  notes                    TEXT
  created_at               TEXT NOT NULL
  updated_at               TEXT NOT NULL

  INDEX (dvo_batch_id, recv_date)
```

### 7.5  Pending events (deferred to "later")

`production_event_pending` — Claude email-funnel agent writes here, not into `production_event` directly. Schema mirrors `production_event` plus `source TEXT NOT NULL CHECK (source IN ('email','manual','import'))`, `source_ref TEXT`, review timestamps. **Defer the migration** until the email-funnel agent ships.

### 7.6  Support tables

```
schema_version      version INTEGER PK, applied_at TEXT
app_settings        key TEXT PK, value TEXT, updated_at TEXT
drift_log           id PK, detected_at, kind, target_id, expected, actual, message,
                    resolved_at, resolved_by    -- append-only; UI surfaces unresolved
```

### 7.7  The ledger functions (Rust, not SQL views)

```rust
pub fn warehouse_ledger_flec(
    conn: &libsql::Connection,
    warehouse_id: i64,
    start_date: NaiveDate,
) -> Result<Vec<FlecLedgerRow>, Error>
// For WHSE 1, 2, 5, 7. Returns per-(grade, side) running balance in flec count.

pub fn dvo_batch_ledger(
    conn: &libsql::Connection,
    dvo_batch_id: i64,
) -> Result<DvoBatchLedger, Error>
// For WHSE 3. Returns interleaved receipts + outflows in date order, kg running balance,
// live transit_loss + yield_loss summary numbers.
```

Both lock-in tested with a `tmp_db` fixture and minimal seed data. Both follow the §4.7 "always show solution" rule by returning the input components alongside the computed values.

---

## §8 — Reference architecture: 12 patterns to apply

These come from `docs/reference/architecture-reference.md`. **Apply them all** in this build. The reference doc has the long-form rationale; one-liners here as a checklist.

1. **Append-only migrations.** A `_MIGRATIONS` list of SQL strings, indexed by version. Never edit a previous migration; always append.
2. **Surrogate `id` + natural columns.** Never compose a primary key from free-text. `unique_tag` is a persisted/computed column for audit and export only.
3. **Canonicalize-at-write.** Every categorical free-text field has a `canonicalize_<field>` function applied at every write site. Lock-in tests for every variant.
4. **State machines as closed enums.** `flec_stat`, `dvo_batch.status`, any future status flag = closed enum + `compute_*_transition(old, new)` helper. No scattered `if status == "foo"` conditionals.
5. **Dirty flag on mutable rows.** Cheap to add now, expensive to add later. Set 1 on every write; cleared to 0 only after the WHOLE downstream operation lands.
6. **Drift log table.** Append-only. Every silent-failure path writes here. UI surfaces with a "Resolve" action.
7. **Transaction context manager.** Every multi-statement write inside a `with_transaction { ... }` (or Rust equivalent).
8. **App settings k/v table.** UI-toggleable knobs, defaults, period boundaries.
9. **User-confirmed mutations for external-event side effects.** External signals never auto-update inventory state. Surface, confirm, then apply.
10. **Multi-line same-key is a recurring shape.** Code that groups by `(warehouse_id, grade_id, side)` MUST use a `Vec`/`HashMap<K, Vec<V>>`, never `HashMap<K, V>`.
11. **Sync orderings are load-bearing.** If we add any push-to-X flow later: pull before push, always.
12. **Pre-launch checklist.** Idempotent migrations, canonicalize at every write, state-machine transitions tested, every multi-write in a transaction, drift log populated by every silent-failure path, lock-in tests for invariants, nightly DB backup.

### Anti-patterns (verbatim from the sister-app's bug ledger)

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

### Step 1 — Schema extraction MD ✅ **DONE 2026-05-07**

Output: `docs/schema-extraction.md`. Surveyed the workbook, walked through corrections with Renzo, captured the corrected three-flow model + DVO subsystem.

### Step 2 — Repo scaffold *(next session)*

Output: a working Tauri v2 + Rust + SvelteKit + libSQL boot. Doesn't need to do anything functional yet — launches a window with a "Hello" page that successfully connects to a local libSQL file.

Includes:
- `src-tauri/` (Rust) with `main.rs`, `db/mod.rs` (connection + migrations runner that applies to remote first), `commands/` (placeholder), `canonicalize.rs` (one fn per categorical), `ledger.rs` (function stubs).
- `src/` (SvelteKit) with adapter-static config, Tailwind v4 + shadcn-svelte initialized, one route.
- `migrations/v1.sql` — the schema from §7.
- `seed.sql` — canonical lookup-table values.
- `.env.example`, README documenting BYO-Turso setup + keyring credential flow.
- `tauri.conf.json` configured for SPA mode.
- `vite.config.ts` with the correct plugin order (SvelteKit first, Tailwind second).
- `Cargo.toml` and `package.json` with all deps pinned.

### Step 3 — First vertical slice

A single screen: "Log a production event." End-to-end through the whole stack.
- SvelteKit form with superforms + zod validation, deriving `unique_tag` live as fields fill.
- `invoke('create_production_event', payload)` to a Rust command.
- Rust canonicalizes, validates, inserts into libSQL inside a transaction.
- libSQL syncs to Turso.
- Form returns success, list refreshes.

### Step 4 — Excel data migration

Output: `scripts/migrate_from_xlsb.py`. One-shot Python script. Reads `2025 CI PRODUCTION V2.xlsb`:
- `Production` rows → `production_event`
- `DVO IN` rows → `dvo_receipt` (with `dvo_batch` records implied/created from BATCH/SIDE codes)
- `PC WHSE *` STARTING blocks → `warehouse_opening_balance`
- The 1 confirmed-mistake duplicate row → import one, drop the other into `drift_log`

### Step 5 — Warehouse + DVO ledger views

Output: Rust `warehouse_ledger_flec` + `dvo_batch_ledger` functions + SvelteKit routes that call them and display the results with §4.7 information density. Lock-in tests asserting exact balances against fixtures.

### Step 6 — Iterate

Whichever screen hurts most in daily use comes next. Ask Renzo, don't guess.

### Later (defer until asked)

- Management dashboard URL — separate small app reading the same Turso DB.
- Claude email-funnel agent — separate Python daemon. Anthropic SDK + Gmail API + `production_event_pending` writes.
- RC Inventory module — separate workbook, separate schema-extraction pass, separate module.

---

## §10 — Reference files (in `docs/reference/`)

| File | What it is | When to open it |
|---|---|---|
| `charcoal-context-extraction-task.md` | Renzo's original brief. Some details superseded by post-walkthrough corrections in `docs/schema-extraction.md`. | Background reading; not authoritative anymore. |
| `architecture-reference.md` | The sister-app's distilled architectural patterns. Use the patterns, NOT the stack. | Anytime applying a pattern (canonicalize, state machine, drift log, etc.). |
| `2025 CI PRODUCTION V2.xlsb` | The live production workbook for CI. **In scope.** | Step 4 (migration). Read with pyxlsb for values, convert via LibreOffice + openpyxl for formulas. |
| `CI RC INVENTORY.xlsb` | The RC inventory workbook. **Reference only — out of scope.** | Don't open during the production-module build. Future module's source. |

If you need pyxlsb / openpyxl / pandas:
```bash
pip3 install --break-system-packages pyxlsb pandas openpyxl
```

For verbatim formulas, install LibreOffice (`brew install --cask libreoffice`) and convert .xlsb → .xlsx first. The Step 1 extraction worked from values only and confirmed the formula classes by their value patterns; a future session needing exact formula text should install LibreOffice.

---

## §11 — Open questions for Renzo

All original questions are now closed. Summary of resolutions (for posterity):

- **Q1 (BATCH = month?)**: not strictly equal at boundaries — same-day batch transitions occur (closing MAY by emptying the tank, then starting JUNE on the same physical day). Keep BATCH as a separate TEXT column.
- **Q2 (CCC RECV semantic)**: row's logging date — partner reports OR CI's own draw-down events. Renamed `recv_date`.
- **Q3 (C1–C4 / RK1–RK4)**: partner's 4 crushers and 4 rotary kilns. Renamed via `partner_equipment` lookup.
- **Q4 (validation rules)**: closed via Renzo's walkthrough — see §6.7 of `docs/schema-extraction.md` for the full validity matrix and the SRC↔PLANT pairing rules.
- **Q5 (canonical form)**: `WHSE 7` is canonical. `W6`/`W7` in the `WHSE` column are cosmetic and migrate to NULL.
- **Q6 (`flec_stat` state machine)**: closed — `flec_stat` is a legacy column from when Renzo manually copied entries into WHSE sheets. Now that WHSE sheets auto-fill, the column is dead. codo imports it as a nullable TEXT for historical fidelity but does NOT validate or write to it. If we revive a review-pending workflow it'll be a real new state machine in a future migration.
- **Q7 (kg balance for WHSE 1/2/5/7?)**: no — only WHSE 3 runs in kg, per batch.
- **Q8 (duplicate UNIQUE TAG)**: confirmed mistake. Migration imports one, drift-logs the other.
- **Q9 (`NOVEMBER2025RIGHT` etc.)**: DVO batch codes; sequester via `dvo_batch_id`.
- **Q10 (first-launch UX)**: closed — Path B (auto-create via Turso Platform API). See §5 gotcha #12 for the flow.
- **Q11 (DVO outflows separate table?)**: no, they stay in `production_event` with `dvo_batch_id` FK; only DVO inflows get their own table (`dvo_receipt`).
- **Q12 (verbatim formulas needed?)**: no, value-pattern-confirmed picture is sufficient.

If a new question emerges during Step 2+ implementation, document it here as Q13+ with what it blocks and the working default.

---

## §12 — Glossary (charcoal-domain terms)

| Term | Meaning |
|---|---|
| **CCC** | Just the partner company's codename. Has no domain meaning. The `CCC RECV` column is the row's logging date; `CCC / FLEC` mixes "what CI did" (FLEC) with "which partner equipment got it" (C1–C4 crushers, RK1–RK4 kilns). codo renames these to remove the legacy baggage. |
| **FLEC** | Short for "flecon bag." The unit CI ships finished charcoal in. `FLEC AMT = 30` means 30 bags. WHSE 1/2/5/7 ledgers run in flec count, not kg. |
| **GRADE** | Product grade. Values: `3X50`, `2X6`, `3.5`, `4X8`. The numbers refer to bag dimensions or sieve sizes (industry-specific). |
| **PLANT** | CI's physical production plant on the Cebu site. `W6` (multi-tank), `W7` (single-tank), `DVO` (Davao plant — appears on outflow rows from WHSE 3 representing partner takes of Davao product). The legacy value `W6 / W7` (87 rows in observed data) was Renzo's earlier attempt to track which CI plant produced flec that ended up in inventory — abandoned as unsustainable. Migration normalizes `W6 / W7` to `NULL` when the source is `FLEC` (origin plant is unknown once bagged), and to the source's home plant otherwise. |
| **WHSE** (column) | Destination warehouse for the event. Real values: `WHSE 1`, `WHSE 2`, `WHSE 3`, `WHSE 5`, `WHSE 7`. Other values (`W6`, `W7`) in this column are cosmetic — pre-auto-fill noise — and migrate as NULL. |
| **SRC** | Source location of the product in this event. The truthful field for "where did this come from." Tanks (`TNK 1..4`), the W7 plant tank (`W7`), direct W6 plant output (`W6`), already-bagged inventory (`FLEC`), or Davao container (`DVO`). |
| **TNK 1..4** | The four tanks at plant W6. 3X50 charcoal accumulates here from the plant; partner draws from them daily. |
| **W6 (as `SRC`)** | Direct W6-plant output, **bypassing tanks**. Typical for non-3X50 grades (3.5, 2X6, 4X8) that come straight out as flec bags. Note: W6 the plant ALSO feeds the four tanks, but those events use `SRC = TNK n`, not `SRC = W6`. |
| **W7 (as `SRC`)** | The single tank at plant W7. Used for both tank-stage and direct-plant events at W7 (W7 doesn't have a separate "plant_direct" code in observed data). |
| **Tank assignment** | Conventional mapping of `(plant, tank, prod_date, shift, batch)` to a particular production lot. **Never logged explicitly** — derived from rows that reference the tank. |
| **SHIFT** | Production shift. `M` = Morning, `E` = Evening, `N` = Night. Only `M` observed in real data. |
| **WHSE SIDE** | For WHSE 1/2/5/7: physical side (`LS` = Left Side, `RS` = Right Side). For WHSE 3 outflow rows: hijacked to carry the DVO batch code (e.g. `NOVEMBER2025RIGHT`); codo migrates these into `dvo_batch_id`. |
| **FLEC STAT** | Status of the bagging operation. Observed: `DONE`. Other values likely exist; Q6 open. |
| **DVO** | Davao. The sister branch ICTC's location. In this workbook, `DVO` shows up in three places: as a `PLANT` value on outflow rows from WHSE 3, as a `SRC` value (meaning "from Davao container"), and as the prefix on the DVO IN / DVO OUT sheets. |
| **DVO IN** | The sheet (and the corresponding flow) where Davao-produced charcoal arrives at WHSE 3 in container vans via Gothong shipping. **In scope** — codo absorbs into `dvo_receipt`. |
| **DVO batch / BATCH/SIDE** | A storage period for one side of WHSE 3. Format: `MONTH(start of piling)YEARSIDE`, e.g. `NOVEMBER2025RIGHT` = side started piling November 2025 on the RIGHT side of WHSE 3. First-class entity in codo (`dvo_batch` table) with manual open/close lifecycle. |
| **Transit loss (DVO)** | `(sum_dvo_declared_weight − sum_cebu_declared_weight) / sum_dvo_declared_weight` per batch. How much weight disappears between Davao's bill of lading and Cebu's scale. Computed live, frozen on close. |
| **Yield loss (DVO)** | `(sum_cebu_declared_weight − sum_partner_takes_weight) / sum_cebu_declared_weight` per batch. How much weight disappears between Cebu receipt and partner outflow (moisture, dust, measurement variance, etc.). Computed live, frozen on close. |
| **C1..C4** | Partner's 4 Crushers. A `disposition` value on `production_event` rows means "partner reported feeding our charcoal into their Crusher N." |
| **RK1..RK4** | Partner's 4 Rotary Kilns. Same shape as C1–C4. |
| **RC** | Raw Coal. Upstream raw material. The `CI RC INVENTORY.xlsb` workbook tracks RC stock — out of scope for this build. |
| **UNIQUE TAG** | A 10-field hyphen-concat used by the workbook as a row identifier. Brittle (empty fields produce `--` runs, position-based parsing). codo keeps it as a computed column for audit/export but uses surrogate `id` for joins. |

---

## §13 — Updating this brain

When something here turns out to be wrong, fix it in this file FIRST, then continue coding. The brain is the source of truth, not your conversation memory. A new session in 3 weeks should be able to pick up exactly where the last one left off by reading this top to bottom.

If you add a major architectural decision, give it its own subsection in §4. If you discover a new gotcha, add it to §5. If you finish a roadmap step, mark it complete in §9 and add a one-line "what was actually built" note. If a glossary term gets confirmed by Renzo, remove the "**Confirm.**" hedge.

Memory entries (in the auto-memory system at `~/.claude/projects/-Users-renzosy-CI-ICTC-inventory-app/memory/`) carry across sessions automatically — the brain is for project-level facts that need to live in the repo.
