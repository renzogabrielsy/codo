# codo

Local desktop production-event log for **CI**, a charcoal-manufacturing
company in Cebu, Philippines (with a sister branch **ICTC** in Davao). codo
replaces a six-year-old Excel workbook with a Tauri desktop app, a libSQL
local database, and Turso cloud sync.

The single source of truth for what this is and why is
[`PROJECT_BRAIN.md`](./PROJECT_BRAIN.md). The schema is in
[`docs/schema-extraction.md`](./docs/schema-extraction.md). Read those before
this README — this file is operator-facing setup, not project context.

## Status

Step 2 of the [PROJECT_BRAIN.md §9 roadmap](./PROJECT_BRAIN.md#9--roadmap-order-of-operations)
— a working Tauri v2 + Rust + SvelteKit + libSQL boot with the v1 schema,
canonicalize-at-write functions, and the §7.1 validity matrix locked in by
test fixtures. Step 3 (the "Log a production event" form) is next.

## Stack

| Layer | Pick |
|---|---|
| Shell | Tauri v2 (≥ 2.6) |
| Backend | Rust (stable) |
| DB client | `libsql` Rust crate |
| Local DB → cloud sync | libSQL embedded replica → Turso |
| Frontend | SvelteKit 2 + Svelte 5 (runes), SPA mode via `adapter-static` |
| Styling | Tailwind CSS v4 (`@tailwindcss/vite`) |
| Secrets | OS keyring (macOS Keychain / Windows Credential Manager / Linux Secret Service) |

Why this stack and not Python+Flask: see PROJECT_BRAIN.md §3.

## Prerequisites

- macOS, Linux, or Windows
- **Node.js ≥ 20** with `npm`
- **Rust stable** (install via [rustup](https://rustup.rs))
- A free **[Turso](https://turso.tech) account** for the cloud-mirror DB

## First-launch onboarding (BYO-Turso)

codo never bakes credentials into the binary. On first launch you provide a
**Turso Platform API token** and codo creates a private database in your
account, then drops the Platform token on the floor. Per
PROJECT_BRAIN.md §5 #12.

### One-time setup at Turso's web UI

1. Sign up at [turso.tech](https://turso.tech) (free tier is plenty).
2. Visit your account settings → **API Tokens** → **Create token**.
3. Copy the token (it starts with `eyJ…`). Treat it like a password.

### First launch in codo

1. Run `npm run tauri:dev` (or open the bundled app once Step 2's bundling
   is wired). The app opens to a one-screen onboarding form.
2. Paste the Platform API token. Set a database name (default: `codo`).
3. Click **Create database & continue**. codo will:
   - call the Turso Platform API to create the DB (auto-suffixes the name
     on collision);
   - mint a DB-level auth token;
   - persist the **DB URL + DB-level token** to the OS keyring;
   - **discard the Platform API token** — it is never written to disk;
   - apply migrations to the **remote** DB first (PROJECT_BRAIN.md §5 #1);
   - open a local libSQL embedded replica that auto-syncs to Turso.

After that first run, codo reads the keyring on every launch and never
prompts again. To reset: delete the `codo` entries in your OS keyring
(macOS: open Keychain Access, search for `codo`, delete `turso_db_url`
and `turso_db_token`).

### Why a Platform API token rather than a baked secret

A binary with a baked Turso URL + token would mean: anyone with the binary
owns your DB. Every codo install is per-user — your Platform API token
stays in your account, the DB is yours, the keyring keeps the per-DB token
local-only. If the binary is leaked, the attacker still needs your
Platform API token to spin up new databases on your account, and codo
never had that token in the first place.

## Repo layout

```
PROJECT_BRAIN.md                  ← read first
docs/
  schema-extraction.md            ← read second
  reference/                      ← (gitignored — local snapshots)
migrations/
  v1.sql                          ← initial schema (lookups, dvo, production_event…)
  seed.sql                        ← canonical lookup-table values (idempotent)
src/                              ← SvelteKit frontend (SPA mode)
  routes/+layout.ts               ← ssr=false, prerender=false
  routes/+page.svelte             ← Hello + onboarding gate
  lib/components/Onboarding.svelte
src-tauri/
  Cargo.toml                      ← Tauri 2.6, libsql 0.6, keyring 3, reqwest 0.12
  src/
    lib.rs                        ← Tauri entry, AppState, command registry
    canonicalize.rs               ← one canonicalize_<field> per categorical
    direction.rs                  ← IN/OUT helper (§4.4)
    validation.rs                 ← validity-matrix gate (§7.1 + §7.2)
    ledger.rs                     ← function shapes (Step 5 lands bodies)
    db.rs                         ← migrations runner (remote-first)
    credentials.rs                ← keyring persistence
    turso_platform.rs             ← onboarding API client (§5 #12)
    commands.rs                   ← Tauri commands (is_onboarded, onboard_turso…)
    error.rs                      ← CodoError + serializer for the JS bridge
  tests/
    validity_matrix.rs            ← integration tests covering §7.1 cells
```

## Local development

```bash
# install JS deps
npm install

# run Rust tests (canonicalize unit tests + validity-matrix integration tests)
( cd src-tauri && cargo test )

# launch the desktop app in dev mode
npm run tauri:dev
```

`cargo test` does not require a Turso connection — every test boots an
in-process libSQL DB in a tmp dir and runs the migrations against it.

## Contributor note: always show the solution

Information density is a guardrail, not a style choice
([PROJECT_BRAIN.md §4.7](./PROJECT_BRAIN.md#47--always-show-the-solution-information-density)).
codo is replacing the source-of-truth spreadsheet — every UI surface must
show the derivation alongside the result, and every backend function should
return inputs alongside outputs. Ledger views render `starting + ins − outs
= current`, KPIs surface their input rows, forms preview derived fields
live as you type. The `RunBalComponents` and `LossMetric` structs in
[`src-tauri/src/ledger.rs`](src-tauri/src/ledger.rs) are the canonical
example — both carry the inputs that produce the computed value, not just
the value alone.

When in doubt, surface more data, not less. Simplifying density is easy;
re-deriving missing intermediates from a finished UI is not.

## License

TBD. The repo is public to support eventual commercialization (open-source
app + hosted offering); no real production data ever lives in the repo —
see PROJECT_BRAIN.md §4.1 for the public-repo / private-data discipline.
