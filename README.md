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

## Two ways to run codo

**Day-to-day: a real .app installed in /Applications.**

```bash
# one-time: produce the .app and install it
npm install
npm run tauri:build
cp -R src-tauri/target/release/bundle/macos/codo.app /Applications/

# from then on: open from Launchpad / Dock / Spotlight, like any app
```

**In-app update — Check for updates** (tauri-plugin-updater + GitHub Releases).

Open codo → click **Check for updates** in the Hello panel. codo queries
`https://github.com/renzogabrielsy/codo/releases/latest/download/latest.json`,
compares against the running version, downloads + verifies the signature +
swaps the binary, then relaunches. Update payloads are signed with a
self-managed minisign keypair — the public key is embedded in
`tauri.conf.json`, the private key lives only on the operator's Mac at
`~/.tauri/codo.key`.

To cut a release:

```bash
./scripts/release.sh 0.0.2 --notes "what shipped"
# Builds + signs locally, pushes the tag + bump commit, creates a DRAFT
# GitHub Release with all artifacts. Open the URL it prints, click Publish.
# Installed copies see the update on next "Check for updates".
```

See [Release process](#release-process) below for the one-time keypair setup.

**Pull-from-git fallback** (still.hobbies-style, used while developing):

```bash
./scripts/update_codo.sh           # ff-pulls the current branch
./scripts/update_codo.sh main      # or check out + pull a specific branch first
```

The script ff-pulls, runs `npm install`, runs `npm run tauri:build`, replaces
`/Applications/codo.app`, and relaunches. Cold rebuilds take ~5–10 min;
warm rebuilds (only Rust source changed) ~1–2 min. Useful for working
against an unreleased branch without cutting a tag.

> **About macOS Gatekeeper.** Without an Apple Developer ID ($99/yr), the
> first launch of a freshly-installed codo build shows a "developer can't be
> verified" dialog — right-click the app → Open → Open. The auto-updater
> itself doesn't need Apple's signing; that's a separate, optional concern
> (see PROJECT_BRAIN.md §5 #7).

**Developer mode: hot-reload while editing.**

```bash
npm run tauri:dev
```

Boots vite + cargo run + opens a window pointing at the dev server. Edit
a `.svelte` file → instant reload. Edit a `.rs` file → cargo rebuild +
window relaunch. Useful while coding; not how you'd run codo for daily
ops.

## Tests

```bash
( cd src-tauri && cargo test )
```

`cargo test` does not require a Turso connection — every test boots an
in-process libSQL DB in a tmp dir and runs the migrations against it.

## Release process

Cutting a release publishes a signed `.app.tar.gz` + a `latest.json`
manifest to a GitHub Release. Installed copies of codo poll that endpoint
and self-update. The whole pipeline runs locally via `scripts/release.sh`
— no CI, no GitHub Actions secrets, no `workflow` scope on your gh
token. (codo is single-operator on a single Mac; CI overhead doesn't pay
for itself yet.)

### One-time keypair setup

1. **Generate the Tauri signing keypair on this Mac.**
   ```bash
   npx tauri signer generate -w ~/.tauri/codo.key
   ```
   Use a passphrase. Save it. Save the key. Treat both like passwords —
   if you lose them, you can't sign new updates and have to rotate the
   public key (which orphans every installed copy).

2. **Save the passphrase to disk** so the release script can find it:
   ```bash
   echo 'YOUR_PASSPHRASE' > ~/.tauri/codo.key.passphrase
   chmod 600 ~/.tauri/codo.key.passphrase
   ```
   Or `export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=…` in your shell rc —
   the script picks up either.

3. **Public key already embedded.** See `plugins.updater.pubkey` in
   `src-tauri/tauri.conf.json`. If you ever rotate the keypair, update
   that field; **all currently-installed codo binaries will refuse the
   new update** (intentional — it's the signature check working).

### Cutting a release

```bash
./scripts/release.sh 0.0.2
./scripts/release.sh 0.0.2 --notes "Step 3: production-event form lands"
```

What it does:

1. Verifies the working tree is clean and the tag isn't already used.
2. Bumps the version in `src-tauri/Cargo.toml` AND `src-tauri/tauri.conf.json`
   in lockstep (the updater compares against `tauri.conf.json`'s version
   field — they MUST match or the auto-update silently no-ops).
3. Commits the bump, tags it `vX.Y.Z`.
4. Runs `npm run tauri:build` with the signing env wired. Produces
   `codo.app`, `codo_X.Y.Z_aarch64.dmg`, `codo_X.Y.Z_aarch64.app.tar.gz`,
   `codo_X.Y.Z_aarch64.app.tar.gz.sig`.
5. Generates `latest.json` from those outputs (the manifest the updater
   plugin fetches).
6. Pushes the tag + bump commit to `origin`.
7. Calls `gh release create --draft` with all artifacts attached, prints
   the URL.
8. **You open the URL, edit the release notes, click Publish.** Drafts
   never reach the auto-updater; only published releases do.

After publish, existing codo installs see the update on next click of
**Check for updates** in the Hello-page panel.

### Common issues

- **`gh: command not found`** → `brew install gh && gh auth login`.
- **Working tree not clean** → commit or stash.
- **Tag already exists** → bump the patch (e.g. 0.0.2 → 0.0.3).
- **Build fails to sign** → `~/.tauri/codo.key.passphrase` is wrong, or
  the env var is set to the wrong value.
- **`Check for updates` says "no update"** when you expected one →
  the GitHub Release is still in DRAFT state. Click Publish.

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
