# Inventory app architecture reference

A distillation of the **still.hobbies inventory app** — a native macOS inventory + sync tool — for use as architectural reference when building a similar app for a different domain. Strip the Shopify-specific bits; keep the patterns. Most decisions here are battle-tested in production over ~6 months and many of them are documented as lessons in the project's bug ledger.

---

## What this app actually is

A single-operator desktop tool for managing inventory of ~10k+ SKU-level items (Pokemon TCG cards, but the design generalizes). Native macOS feel via PyWebView; entirely offline-first; one-click sync to an external commerce backend (Shopify in our case, irrelevant for the new project). The app is the **sole authoritative inventory store** for the operator. The remote backend mirrors local state via push/pull, never the reverse.

Daily operator workflow:
1. Open the app → it pulls latest code from git, opens the native window
2. Add new items via typeahead-search + bulk-paste-grid
3. Local SQLite is canonical; remote backend trails via Sync clicks
4. Pricing + analytics computed locally; pushed when ready
5. Orders pulled from remote → picked from physical bins → fulfilled

For your production + inventory context, the equivalent might be: factory floor logs production batches → inventory levels updated locally → sales/distribution backend pulls when ready.

---

## Tech stack

### Backend
- **Python 3.11+** — modern syntax, type hints, no compatibility ceremony
- **Flask** — bound to `127.0.0.1` only; serves JSON API + SSE event stream + static frontend
- **SQLite (WAL mode)** — single-file database in `~/Library/Application Support/<App Name>/`, survives app updates
- **Standard library** for everything else: `urllib`, `json`, `subprocess`, `threading`, `socket`. No SQLAlchemy, no ORM, no Celery, no Redis.
- Per-thread SQLite connection pool via `threading.local()` — Flask serves requests in worker threads, each gets its own conn

### Frontend
- **Vanilla HTML/CSS/ES modules** — no build step, no React, no bundler
- **Hash-routing SPA** (`#/inventory`, `#/sync`, etc.) — single-page app, ~2k lines of plain JS across views
- **Server-Sent Events (SSE)** for long-running operation progress (sync log streaming)
- **IndexedDB** in the browser for image caching (~10k card art thumbnails)

### Window + distribution
- **PyWebView** wraps Flask in a native Cocoa window (no Electron, no WebView2)
- **Shell-script launcher** (`launcher.sh`) → wrapped as `.app` bundle by `install_launcher.sh`. Every launch:
  1. `git pull --quiet origin main` (silent self-update)
  2. Ensure `.venv/` has correct deps
  3. Boot Flask + PyWebView
- Optional **py2app** bundle for non-developer Macs, but the dev-workflow .app is faster to ship

### Operations
- All env vars from `.env` (repo root or `~/Library/Application Support/<App>/`)
- Logs to `~/Library/Logs/<App>.log`
- Native menu items via PyWebView for "Reload (git pull + restart)" and "Force Restart"

### Why these choices

**Why not Django/FastAPI**: overkill for single-operator desktop tool. Flask is 200 lines of boot, fits in your head. JSON API + SSE is all we need.

**Why SQLite not Postgres**: single-operator means single-writer. WAL mode handles concurrent reads. Backups are `cp inventory.db`. No daemon to keep alive. File survives app updates because it lives outside the app bundle.

**Why no React**: 2k lines of vanilla JS with hash routing is faster to write, faster to deploy (no bundler), and easier to debug (no source maps, no JSX, no virtual DOM). The view files (`add-card.js`, `inventory.js`, etc.) are self-contained — open one and you understand the entire screen.

**Why git-pull-on-launch**: shipping = `git push origin main`. No installer, no auto-updater, no signing dance. Works because the app runs locally and the operator owns the git checkout.

---

## Module structure (this is the skeleton you should copy)

```
app/
├── <module_name>/
│   ├── main.py              # Flask + PyWebView entry point
│   ├── config.py            # paths, env loader, version
│   ├── db.py                # SQLite schema + migrations + transaction context manager
│   ├── server.py            # Flask app factory + JSON API routes + SSE bus
│   ├── <domain>.py          # one module per domain (catalog, inventory, orders, pricing, …)
│   ├── sync.py              # orchestration (push, pull, drain queue)
│   ├── normalize.py         # canonicalize free-text fields at write time
│   ├── polling.py           # background daemon thread for periodic remote pulls
│   └── static/
│       ├── index.html       # sidebar layout + hash routing
│       ├── app.js           # router + el() + api() helpers
│       ├── styles.css
│       └── views/
│           ├── add-card.js
│           ├── inventory.js
│           ├── sync.js
│           └── ...
├── build/
│   ├── launcher.sh          # daily launch sequence (git pull + venv + boot)
│   ├── install_launcher.sh  # creates .app bundle in ~/Applications/
│   └── setup.py             # py2app config (optional, for distribution)
├── docs/
│   └── *.md
└── tests/
    ├── conftest.py          # `tmp_db` fixture
    └── test_*.py
```

---

## Database design patterns

### Append-only migrations

`db.py` defines a list of SQL strings, one per schema version:

```python
_MIGRATIONS: list[str] = [
    # v1
    """CREATE TABLE cards (...); CREATE INDEX ...""",
    # v2
    """ALTER TABLE inventory ADD COLUMN bin_position INTEGER;""",
    ...
]

def bootstrap(conn):
    current = _current_version(conn)  # reads schema_version table
    for idx, sql in enumerate(_MIGRATIONS, start=1):
        if idx > current:
            conn.executescript(sql)
            conn.execute("INSERT INTO schema_version VALUES (?)", (idx,))
```

**Never edit a previous migration.** Always append. The migration list is a permanent contract; rewriting v3 after users have v3 applied creates divergent schemas.

Past schema migrations on this project (15 versions in 6 months): bin_position, dirty flag, app_settings k/v, last_pushed_price (for "stale" indicator), illustrator metadata, order lifecycle state (inventory_restored + edit_pending). Each one earned its place from a real bug or feature.

### The dirty flag pattern

Every inventory row has `dirty INTEGER DEFAULT 1`. Local edits set `dirty=1`; sync clears to 0 after pushing successfully. Sync **only fetches+diffs handles with at least one dirty row** (unless force=True). Saves massive API churn — a 10k-row catalog with 50 dirty rows means 50 fetches, not 10k.

The state-machine rule: dirty/clean is binary, not "in-progress." Don't transition to clean until the WHOLE write operation lands. We learned this the hard way — see "premature mark_synced" in the bug ledger.

### `last_pushed_*` columns

Every inventory row has `last_pushed_price REAL` and `last_pushed_at TEXT`. The mental model: this is the **receipt of what we last sent the remote backend**. Used for:
- Drift detection (Shopify drifted from us → log to drift_log)
- Stale indicator in UI (∇ icon when computed-suggested-price differs from last_pushed)
- Atomicity (defer this update until after the bulk push succeeds)

For your production tracker: the equivalent might be `last_reported_qty` to a downstream sales system, or `last_audited_at` for shrinkage tracking.

### Drift log table

```sql
CREATE TABLE sync_drift_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    detected_at TEXT NOT NULL,
    kind TEXT NOT NULL,          -- 'qty_mismatch', 'price_drift', 'verify_missing', etc.
    handle TEXT,
    listing_name TEXT,
    shopify_qty INTEGER,
    local_qty INTEGER,
    message TEXT
);
```

Append-only. UI surfaces unresolved entries with a "Resolve" action. **Every silent failure should produce a drift entry.** It's the difference between "things are wrong and you don't know" and "things are wrong and the app is yelling at you."

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

Wrap every multi-statement write in `with db.transaction(conn): ...`. SQLite is fast enough that you should never NOT use a transaction for bulk writes.

### App settings (k/v table)

```sql
CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT);
```

For UI-toggleable knobs (polling interval, pricing tier overrides, blend strategy, theme). Way simpler than a settings file — you get atomicity for free, and you can join settings into queries.

---

## Frontend patterns

### Vanilla SPA with hash routing

```javascript
// app.js (~50 lines for the router)
window.addEventListener('hashchange', route);
async function route() {
    const hash = window.location.hash.slice(1) || '/inventory';
    const main = document.getElementById('main');
    main.innerHTML = '';
    if (hash.startsWith('/inventory')) await renderInventory(main);
    else if (hash.startsWith('/sync')) await renderSync(main);
    // ...
}
```

Each view is a separate `views/<name>.js` module that exports a `render*` function. Views are self-contained — they create their own DOM, attach their own event listeners, fetch their own data via `api.get(...)`.

### `el()` helper for DOM construction

```javascript
function el(tag, attrs, ...children) {
    const node = document.createElement(tag);
    if (attrs) for (const [k, v] of Object.entries(attrs)) {
        if (k === 'class') node.className = v;
        else if (k === 'style') node.setAttribute('style', v);
        else node.setAttribute(k, v);
    }
    for (const c of children.flat()) {
        if (c == null) continue;
        node.appendChild(typeof c === 'string' ? document.createTextNode(c) : c);
    }
    return node;
}
```

Replaces JSX. Pure data → DOM. Faster than React for our scale (single-operator UI).

### `api` helper

```javascript
export const api = {
    async get(path) {...},
    async post(path, body) {...},
    async put(path, body) {...},
    async patch(path, body) {...},
    async del(path) {...},
};
```

Wraps `fetch` with consistent error handling + JSON serialization. Surfaces server-side `error` field as `err.message` so toasts read as English.

### SSE event bus

For long-running operations (sync, bulk import, pricing recalc), publish progress events to an in-memory bus, expose via `/api/sync/log` (text/event-stream). Frontend opens an EventSource on tab mount, renders log lines + progress bar in real time.

```python
class SyncLogBus:
    def __init__(self):
        self._subscribers = []
        self._history = []  # last 500 events for replay on (re)connect

    def publish(self, event):
        self._history.append(event)
        for q in self._subscribers:
            q.put(event)
```

For your production app: any operation that takes > 2 seconds should publish progress events. Operators tolerate slow operations if they can SEE progress; they assume crashed if they can't.

---

## State machine pattern

For order status, batch status, anything multi-state:

1. Define **closed enum** (string constants) — `STATUS_PENDING`, `STATUS_FULFILLED`, etc.
2. **`compute_status_from_X` helper** — derives the state from raw external input, applying priority rules
3. **`compute_status_transition(old, new)` helper** — enforces directional rules (terminal states never downgrade)
4. **Test every transition** — `STATUS_PENDING → STATUS_CANCELLED` allowed, `STATUS_CANCELLED → STATUS_PENDING` rejected

The rule: **never use inline `if status == 'foo'` conditionals scattered across the codebase.** Centralize the state machine.

We learned this the hard way: `orders.status` started as a 2-state field (pending, fulfilled) and silently absorbed 5 other states as "pending" because no code recognized them. Took 6 months and a real-money incident to clean up.

---

## Canonicalize-at-write boundaries

For any free-text field that gets composed into a primary key OR exposed as a tag/category:

1. Define `canonicalize_<field>(raw: str) -> str` in `normalize.py`
2. Closed enum lookup: lowercased input → canonical form
3. Apply at **every write site** (insert, update, import, manual edit)
4. Lock-in test: for every input variant, assert it maps to the canonical
5. Backfill script: walk existing rows, apply canonical, merge duplicates

Why: if your data can arrive in multiple casings ("Double Rare" vs "Double rare") or with whitespace drift, you'll silently get duplicate categories in any UI that displays them. Canonicalize once at write, never at read.

For TCG we have ~30 rarity values + 20 variant patterns. For your production app, equivalents might be: lot codes, supplier names, product categories, anything where humans type values into a form.

---

## User-confirmed inventory mutations

**The most important real-money rule we learned.**

For inventory side-effects of EXTERNAL events (orders cancelled, refunds, edits from a remote system), NEVER auto-apply to local inventory. Instead:

1. Detect the event (status change, line item diff, etc.)
2. Set a flag in the DB (`needs_review`, `inventory_restored=0`, `edit_pending=1`)
3. UI shows a banner with explicit confirm/dismiss buttons
4. Operator clicks "Apply" only after physically verifying the inventory matches the proposed state

**Why**: we can't know whether a cancelled item is physically intact, damaged, or moved. Auto-restoring inventory based on a remote signal corrupts accounting in those cases. The operator's eyes on the bin is the source of truth.

For your production app: if a downstream system reports "lot returned," don't auto-add it back to inventory. Surface it; let the QC inspector decide.

---

## Sync orchestration (where applicable)

For an inventory app that mirrors to a remote backend:

```
sync(force=False):
    1. pull catalog updates from remote (read-only fetch, no local writes besides catalog)
    2. pull order/transaction updates from remote (causes local inventory decrement)
    3. drain push queue (any locally-queued mutations to remote)
    4. push local-dirty inventory to remote
    5. ensure remote-side derived structures (collections, categories, etc.)
    6. POST-SYNC VERIFY: re-fetch what we just pushed, diff against intent, log mismatches
```

**Order matters.** Pull before push, always. Push always operates on post-pull state, never pre-pull. We had a phantom-restock race because push ran before pull — fixed by reordering.

**Atomicity within push**: defer "mark dirty=0" until AFTER the bulk write succeeds. Half-completed writes leave dirty intact for next sync to retry.

**Idempotency**: every sync operation should be safe to re-run. Use UPSERT, never INSERT.

---

## Polling vs webhooks

For real-time-ish updates from a remote system, polling is usually the right answer:

```
Background daemon thread:
  every N seconds:
    if polling.enabled:
      try_acquire_sync_slot()
      pull_orders()
      release_slot()
    sleep N
```

**Why polling > webhooks for small ops**: no public URL, no tunnel, no firewall holes, no auth token. Just outbound calls. Latency is `N` seconds (60s is fine for most flows).

Use webhooks only when you NEED sub-second latency (e.g., overselling protection on a busy storefront). Otherwise polling is simpler, more reliable, easier to debug.

For your production app: if you have a downstream sales system, polling is almost certainly enough. Don't bother with webhook infrastructure unless you're shipping > 10 orders/min.

---

## Pre-launch checklist (battle-tested)

Things to verify before any inventory app touches real money:

1. **Schema migrations are idempotent and ordered.** Running bootstrap() twice is a no-op the second time.
2. **All free-text fields that become categories are canonicalized at write time.**
3. **State machines have closed enums + transition rules + tests for every transition.**
4. **Every multi-step write is wrapped in `with db.transaction(conn):`.**
5. **Every external-event detector surfaces in UI for user confirmation; never auto-applies to inventory.**
6. **Sync verifies what it pushed.** Don't trust 200 OK; check the value actually wrote.
7. **Concurrency guards on every fire-and-forget endpoint.** A double-click of Sync should return 409, not race.
8. **Drift log table exists and is populated by every silent-failure path.**
9. **Test suite covers happy paths AND known topology shapes** (multi-line same-key, pre-migration data, etc.).
10. **Backups: nightly `cp inventory.db <backup-dir>` cron, retain 30 days.**
11. **Lock-in tests for invariants** (e.g., "every order has a status in the closed enum"). These fail loudly when anyone breaks the contract.

---

## Anti-patterns (what NOT to do, learned from real bugs)

1. **Don't transition state flags based on partial completion.** Mark dirty=0 only after the whole operation lands.
2. **Don't compose primary keys from raw free-text.** Canonicalize the components first.
3. **Don't use `{key: line}` dicts for line-item-like data.** Use `defaultdict(list)`. Multi-line same-key is a recurring shape.
4. **Don't auto-apply external-event side effects to inventory.** User confirmation only.
5. **Don't rely on platform defaults for integrity-critical fields.** Set them explicitly.
6. **Don't paginate narrowly.** Use ranking within the loop, not natural sort order.
7. **Don't trust upstream data verbatim.** Pair every ingestion with a canonicalize layer.
8. **Don't skip telemetry on silent failures.** Drift log entries make invisible bugs visible.
9. **Don't optimize prematurely.** Vanilla JS, no ORM, no caching layer. Add only when measured.
10. **Don't auto-sync to remote.** User clicks Sync. Background polls only for READS, never WRITES.

---

## Testing strategy

- **Pytest with `tmp_db` fixture** — each test gets a fresh SQLite file, schema applied, no shared state
- **Lock-in tests for invariants** — assert critical contracts (status in enum, dirty flag transitions, canonical form preserved)
- **Integration tests for orchestration** — fake the remote client, run full sync flow, assert end state
- **Regression tests for every fixed bug** — when you fix something, add a test that fails without the fix
- **Tests live in `app/tests/test_*.py`** — pytest-discoverable, fast (~5s for the full suite)

Skip pure-UI tests (Selenium, Playwright). They're slow, flaky, and don't catch real bugs in this kind of app. Use a manual smoke-test checklist before each release instead.

---

## Distribution + ops

For a single-operator app on a Mac, the simplest reliable distribution is:

1. Operator clones the repo to `~/<repo-name>/`
2. Run `bash app/build/install_launcher.sh` once → creates `~/Applications/<App Name>.app`
3. Double-click to launch
4. Every launch: `git pull --quiet origin main`, ensure `.venv/`, boot
5. To ship updates: `git push origin main` from your dev machine. Operator gets the update on next launch.

No installer, no notarization, no auto-updater. The app's self-update is just `git pull`.

For multi-operator: py2app bundle + `make_dmg.py` for distribution. But that's a different problem — code signing, notarization, the full Apple developer dance. Avoid until forced.

---

## Closing — what to keep, what to drop

**Patterns that transfer to ANY inventory app:**
- SQLite + WAL + per-thread connections
- Append-only migrations
- Dirty flag for sync optimization
- Drift log for telemetry
- State machines as closed enums
- Canonicalize-at-write
- User-confirmed mutations for external-event-driven inventory changes
- Vanilla SPA frontend
- SSE for long-running ops
- Polling > webhooks for most flows
- Git-pull-on-launch for self-update

**Patterns that are TCG/Shopify-specific (don't blindly copy):**
- chaos-sort `bin_position` per-card identity (only matters when items are individually unique)
- Multi-finish variant model (Phase 4)
- Discount Shopify Function in Wasm
- Pokemon TCG canonical rarity enum
- Pattern-variant Reverse-Holo domain rule
- The specific TCGcsv / PriceCharting blend strategy

**The lessons that took the longest to learn (in priority order):**

1. **Real-money flows need user-confirmed inventory mutations, not auto-apply.** This single rule prevents 80% of "inventory drift" complaints.
2. **State fields are state machines whether you treat them like one or not.** Closed enum + transition function from day one.
3. **Tests cover happy paths; production exposes the topology.** Document a manual-SQL fallback runbook for when the abstraction doesn't fit reality.
4. **Canonicalize at write boundaries; never at read.** Half-fixes leave the other paths bleeding.
5. **Sync orderings are load-bearing.** Pull before push, always. New phases must respect this.

Pair this doc with your domain-specific Excel + context, and the new app should fit the same shape: SQLite + Flask + PyWebView + vanilla SPA + sync orchestration. Adjust the domain modules (catalog → product spec, inventory → batch tracking, orders → distribution) without touching the architectural skeleton.
