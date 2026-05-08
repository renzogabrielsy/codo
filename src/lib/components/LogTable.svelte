<script lang="ts" module>
  // Pure helpers used by the historical-row template below.
  export function fmtDate(iso: string | null): string {
    if (!iso) return '';
    const m = iso.match(/^(\d{4})-(\d{2})-(\d{2})$/);
    if (!m) return iso;
    return `${parseInt(m[2], 10)}/${parseInt(m[3], 10)}`;
  }
  export function fmtKg(kg: number): string {
    if (kg >= 1000) return (kg / 1000).toFixed(1) + 'k';
    return kg.toFixed(0);
  }
  export function disp(row: {
    disposition_kind: string;
    partner_equipment_code: string | null;
  }): string {
    if (row.disposition_kind === 'flec_bagging') return 'FLEC';
    return row.partner_equipment_code ?? '?';
  }
  /** Disposition pill colour by kind — emerald FLEC / sky Crusher / amber Kiln. */
  export function dispositionBadgeClass(row: { disposition_kind: string }): string {
    const base =
      'inline-block rounded px-1.5 py-0.5 text-[10px] font-bold tracking-wider';
    switch (row.disposition_kind) {
      case 'flec_bagging':
        return `${base} bg-emerald-700/40 text-emerald-200 border border-emerald-600/40`;
      case 'partner_crusher':
        return `${base} bg-sky-700/40 text-sky-200 border border-sky-600/40`;
      case 'partner_kiln':
        return `${base} bg-amber-700/40 text-amber-200 border border-amber-600/40`;
      default:
        return `${base} bg-neutral-700/40 text-neutral-200`;
    }
  }
</script>

<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { tick } from 'svelte';
  import type {
    LookupBundle,
    CreateProductionEventInput,
    ProductionEventRow,
    SourceLocationRow
  } from '$lib/types/codo';

  type Props = {
    lookups: LookupBundle;
    rows: ProductionEventRow[];
    onSaved: (rows: ProductionEventRow[]) => void;
  };
  let { lookups, rows, onSaved }: Props = $props();

  // -------------------------------------------------------------------------
  // Form state — every cell is a typed string. Rust canonicalizes on submit.
  // -------------------------------------------------------------------------

  function todayIso(): string {
    return new Date().toISOString().slice(0, 10);
  }
  function currentMonthName(): string {
    return new Date()
      .toLocaleString('en-US', { month: 'long' })
      .toUpperCase();
  }

  // Sensible defaults — only WT and FLEC need typing on a typical entry.
  let recvDate = $state(todayIso());
  let prodDate = $state(todayIso());
  let batch = $state(currentMonthName()); // e.g. "MAY"
  let shiftCode = $state('M');
  let gradeCode = $state('');
  let sourceCode = $state('');
  let plantCodeOverride = $state('');
  let warehouseCode = $state('');
  let whseSide = $state('');
  let weightStr = $state('');
  let flecStr = $state('');
  let dispositionRaw = $state('FLEC');
  let notes = $state('');

  let submitting = $state(false);
  let err = $state<string | null>(null);

  // -------------------------------------------------------------------------
  // §7.2 plant derivation — forced from source for non-FLEC sources.
  // -------------------------------------------------------------------------

  const sourceByCode = $derived.by(() => {
    const map = new Map<string, SourceLocationRow>();
    for (const s of lookups.source_locations) map.set(s.code, s);
    return map;
  });

  const selectedSource = $derived(
    sourceByCode.get(sourceCode.toUpperCase().trim()) ?? null
  );

  const sourceForcesPlant = $derived(
    selectedSource !== null && selectedSource.kind !== 'warehouse_flec'
  );

  const forcedPlantCode = $derived.by(() => {
    if (!selectedSource || selectedSource.kind === 'warehouse_flec') return '';
    if (selectedSource.plant_id == null) return '';
    const p = lookups.plants.find((x) => x.id === selectedSource.plant_id);
    return p?.code ?? '';
  });

  // What appears in the plant cell — forced value if forced, else operator's
  // override.
  const displayedPlant = $derived.by(() =>
    sourceForcesPlant ? forcedPlantCode : plantCodeOverride
  );

  // -------------------------------------------------------------------------
  // Datalist option sets for type-ahead on bounded cells.
  // -------------------------------------------------------------------------

  const dispositionOptions = [
    'FLEC',
    'C1',
    'C2',
    'C3',
    'C4',
    'RK1',
    'RK2',
    'RK3',
    'RK4'
  ];

  // -------------------------------------------------------------------------
  // Submit / bulk / keyboard
  // -------------------------------------------------------------------------

  let weightInput: HTMLInputElement;

  async function submit() {
    err = null;
    if (submitting) return;

    if (!gradeCode.trim() || !sourceCode.trim() || !batch.trim() || !weightStr.trim()) {
      err = 'fill in batch, grade, source, weight';
      return;
    }
    const wt = parseFloat(weightStr);
    if (!(wt > 0)) {
      err = 'weight must be > 0';
      return;
    }
    const flecN = flecStr.trim() === '' ? null : parseInt(flecStr, 10);
    if (flecN !== null && (!Number.isFinite(flecN) || flecN <= 0)) {
      err = 'flec must be a positive integer (or blank)';
      return;
    }

    const input: CreateProductionEventInput = {
      recvDate,
      prodDate: prodDate.trim() === '' ? null : prodDate,
      batch: batch.trim().toUpperCase(),
      shiftCode: shiftCode.trim() === '' ? null : shiftCode.trim().toUpperCase(),
      gradeCode: gradeCode.trim().toUpperCase(),
      sourceCode: sourceCode.trim().toUpperCase(),
      plantCodeOverride:
        selectedSource?.kind === 'warehouse_flec' && plantCodeOverride.trim()
          ? plantCodeOverride.trim().toUpperCase()
          : null,
      warehouseCode: warehouseCode.trim() === '' ? null : warehouseCode.trim().toUpperCase(),
      dispositionRaw: dispositionRaw.trim().toUpperCase(),
      weightKg: wt,
      flecCount: flecN,
      whseSide: whseSide.trim() === '' ? null : whseSide.trim().toUpperCase(),
      flecStat: null,
      dvoBatchId: null,
      notes: notes.trim() === '' ? null : notes
    };

    submitting = true;
    try {
      const row = await invoke<ProductionEventRow>('create_production_event', { input });
      // Excel-feel: clear only what should change next row. Sticky everything
      // else.
      weightStr = '';
      flecStr = '';
      notes = '';
      onSaved([row]);
      await tick();
      weightInput?.focus();
      weightInput?.select();
    } catch (e) {
      err = String(e);
    } finally {
      submitting = false;
    }
  }

  function clearEditable() {
    err = null;
    gradeCode = '';
    sourceCode = '';
    plantCodeOverride = '';
    warehouseCode = '';
    whseSide = '';
    weightStr = '';
    flecStr = '';
    notes = '';
  }

  function onCellKey(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      submit();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      clearEditable();
    } else if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      submit();
    }
  }

  // -------------------------------------------------------------------------
  // Bulk paste — opens a textarea modal; parses TSV/CSV; submits each row
  // through create_production_events_bulk on the Rust side (Phase B+1, this
  // commit lays the UI groundwork; the Rust command lands in the same chunk).
  // -------------------------------------------------------------------------

  let bulkOpen = $state(false);
  let bulkText = $state('');
  let bulkBusy = $state(false);
  let bulkErr = $state<string | null>(null);

  async function submitBulk() {
    bulkErr = null;
    if (bulkBusy) return;

    const lines = bulkText
      .split(/\r?\n/)
      .map((l) => l.trim())
      .filter((l) => l.length > 0 && !l.startsWith('#'));
    if (lines.length === 0) {
      bulkErr = 'no rows pasted';
      return;
    }

    // Header detection — first line is a header if it contains "RECV" or
    // "BATCH" (case-insensitive).
    const looksLikeHeader = /\b(RECV|BATCH|GRADE)\b/i.test(lines[0]);
    const dataLines = looksLikeHeader ? lines.slice(1) : lines;

    const inputs: CreateProductionEventInput[] = [];
    for (let i = 0; i < dataLines.length; i++) {
      const cells = splitCells(dataLines[i]);
      if (cells.length < 11) {
        bulkErr = `line ${i + (looksLikeHeader ? 2 : 1)}: expected 11+ columns, got ${cells.length}`;
        return;
      }
      const wt = parseFloat(cells[9]);
      if (!(wt > 0)) {
        bulkErr = `line ${i + (looksLikeHeader ? 2 : 1)}: weight must be > 0 (got '${cells[9]}')`;
        return;
      }
      const flecN = cells[10].trim() === '' ? null : parseInt(cells[10], 10);
      inputs.push({
        recvDate: normalizeDate(cells[0]) ?? todayIso(),
        prodDate: normalizeDate(cells[1]),
        batch: cells[2].toUpperCase(),
        shiftCode: cells[3] ? cells[3].toUpperCase() : null,
        gradeCode: cells[4].toUpperCase(),
        sourceCode: cells[5].toUpperCase(),
        plantCodeOverride: cells[6] ? cells[6].toUpperCase() : null,
        warehouseCode: cells[7] ? cells[7].toUpperCase() : null,
        whseSide: cells[8] ? cells[8].toUpperCase() : null,
        weightKg: wt,
        flecCount: flecN,
        dispositionRaw: (cells[11] ?? 'FLEC').toUpperCase(),
        flecStat: null,
        dvoBatchId: null,
        notes: cells[12] || null
      });
    }

    bulkBusy = true;
    try {
      const inserted = await invoke<ProductionEventRow[]>('create_production_events_bulk', {
        inputs
      });
      onSaved(inserted);
      bulkOpen = false;
      bulkText = '';
    } catch (e) {
      bulkErr = String(e);
    } finally {
      bulkBusy = false;
    }
  }

  function splitCells(line: string): string[] {
    // Tab-separated takes precedence (paste from Excel/Sheets); fall back
    // to comma. We don't try to parse quoted CSV — if you need that, paste
    // as TSV instead.
    if (line.includes('\t')) return line.split('\t').map((c) => c.trim());
    return line.split(',').map((c) => c.trim());
  }

  function normalizeDate(s: string): string | null {
    const trimmed = s.trim();
    if (!trimmed) return null;
    // Already ISO?
    if (/^\d{4}-\d{2}-\d{2}$/.test(trimmed)) return trimmed;
    // M/D, M/D/YY, M/D/YYYY
    const m = trimmed.match(/^(\d{1,2})\/(\d{1,2})(?:\/(\d{2,4}))?$/);
    if (m) {
      const month = m[1].padStart(2, '0');
      const day = m[2].padStart(2, '0');
      let year = m[3];
      if (!year) year = new Date().getFullYear().toString();
      else if (year.length === 2) year = '20' + year;
      return `${year}-${month}-${day}`;
    }
    return trimmed; // let Rust reject if it doesn't parse
  }
</script>

<div class="rounded-md border border-neutral-800 bg-neutral-950 overflow-hidden">
  <div class="flex items-baseline justify-between px-3 py-1.5 border-b border-neutral-800">
    <h3 class="text-xs uppercase tracking-wide text-neutral-500">Production log</h3>
    <div class="flex items-center gap-3 text-xs text-neutral-600 font-mono">
      <span>last {rows.length}</span>
      <button
        type="button"
        class="rounded border border-neutral-800 hover:border-neutral-700 px-2 py-0.5 text-neutral-400 hover:text-neutral-200"
        onclick={() => (bulkOpen = !bulkOpen)}
      >
        {bulkOpen ? 'close bulk' : 'bulk paste'}
      </button>
    </div>
  </div>

  <!-- Top-of-table error banner. Loud red so a missed-required-field doesn't
       fall through silently. -->
  {#if err}
    <div class="px-3 py-2 text-xs text-red-100 bg-red-900/60 border-b border-red-700 font-mono">
      ⚠ {err}
    </div>
  {/if}

  <div class="overflow-x-auto">
    <table class="w-full text-xs font-mono">
      <colgroup>
        <col style="width: 24px" /><!-- + indicator -->
        <col style="width: 64px" /><!-- RECV -->
        <col style="width: 64px" /><!-- PROD -->
        <col style="width: 100px" /><!-- BATCH -->
        <col style="width: 44px" /><!-- SH -->
        <col style="width: 70px" /><!-- GRADE -->
        <col style="width: 76px" /><!-- SOURCE -->
        <col style="width: 62px" /><!-- PLANT -->
        <col style="width: 80px" /><!-- WAREHOUSE -->
        <col style="width: 56px" /><!-- SIDE  ← was 38, clipping LS/RS -->
        <col style="width: 90px" /><!-- WEIGHT -->
        <col style="width: 60px" /><!-- FLEC -->
        <col style="width: 70px" /><!-- DEST (DISP) -->
        <col /><!-- NOTES (flex) -->
        <col style="width: 92px" /><!-- ACTION button column -->
      </colgroup>
      <thead class="bg-neutral-900 text-neutral-300 text-[11px] uppercase tracking-wider">
        <tr>
          <th></th>
          <th class="px-2 py-1.5 text-left font-medium">Recv</th>
          <th class="px-2 py-1.5 text-left font-medium">Prod</th>
          <th class="px-2 py-1.5 text-left font-medium">Batch</th>
          <th class="px-2 py-1.5 text-left font-medium">Shift</th>
          <th class="px-2 py-1.5 text-left font-medium">Grade</th>
          <th class="px-2 py-1.5 text-left font-medium">Source</th>
          <th class="px-2 py-1.5 text-left font-medium">Plant</th>
          <th class="px-2 py-1.5 text-left font-medium">Warehouse</th>
          <th class="px-2 py-1.5 text-left font-medium">Side</th>
          <th class="px-2 py-1.5 text-right font-medium">Weight kg</th>
          <th class="px-2 py-1.5 text-right font-medium">Flec</th>
          <th class="px-2 py-1.5 text-left font-medium">Dest</th>
          <th class="px-2 py-1.5 text-left font-medium">Notes</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        <!-- INPUT ROW: visually IS the next event. Loud emerald accent so
             the operator can't miss where to type. -->
        <tr class="bg-emerald-950/40 border-y-2 border-emerald-500/60">
          <td class="px-1 text-center text-emerald-400 font-bold">+</td>
          <td class="px-1 py-1">
            <input
              type="date"
              bind:value={recvDate}
              onkeydown={onCellKey}
              class="cell-input"
            />
          </td>
          <td class="px-1 py-1">
            <input
              type="date"
              bind:value={prodDate}
              onkeydown={onCellKey}
              class="cell-input"
            />
          </td>
          <td class="px-1 py-1">
            <input
              type="text"
              bind:value={batch}
              onkeydown={onCellKey}
              placeholder="MAY"
              class="cell-input uppercase text-neutral-100"
            />
          </td>
          <td class="px-1 py-1">
            <input
              type="text"
              bind:value={shiftCode}
              onkeydown={onCellKey}
              list="dl-shifts"
              placeholder="M"
              class="cell-input uppercase"
            />
          </td>
          <td class="px-1 py-1">
            <input
              type="text"
              bind:value={gradeCode}
              onkeydown={onCellKey}
              list="dl-grades"
              placeholder="3X50"
              class="cell-input uppercase text-violet-300"
            />
          </td>
          <td class="px-1 py-1">
            <input
              type="text"
              bind:value={sourceCode}
              onkeydown={onCellKey}
              list="dl-sources"
              placeholder="TNK 1"
              class="cell-input uppercase text-cyan-300"
            />
          </td>
          <td class="px-1 py-1">
            {#if sourceForcesPlant}
              <input
                type="text"
                value={forcedPlantCode}
                readonly
                tabindex="-1"
                title="auto from source"
                class="cell-input text-neutral-500 cursor-default italic"
              />
            {:else}
              <input
                type="text"
                bind:value={plantCodeOverride}
                onkeydown={onCellKey}
                list="dl-plants"
                placeholder="—"
                class="cell-input uppercase"
              />
            {/if}
          </td>
          <td class="px-1 py-1">
            <input
              type="text"
              bind:value={warehouseCode}
              onkeydown={onCellKey}
              list="dl-warehouses"
              placeholder="—"
              class="cell-input uppercase text-amber-300"
            />
          </td>
          <td class="px-1 py-1">
            <input
              type="text"
              bind:value={whseSide}
              onkeydown={onCellKey}
              list="dl-sides"
              placeholder="—"
              class="cell-input uppercase"
            />
          </td>
          <td class="px-1 py-1">
            <input
              type="number"
              bind:value={weightStr}
              onkeydown={onCellKey}
              bind:this={weightInput}
              step="0.01"
              min="0"
              placeholder="0"
              class="cell-input text-right text-neutral-50 font-semibold"
            />
          </td>
          <td class="px-1 py-1">
            <input
              type="number"
              bind:value={flecStr}
              onkeydown={onCellKey}
              step="1"
              min="0"
              placeholder=""
              class="cell-input text-right"
            />
          </td>
          <td class="px-1 py-1">
            <input
              type="text"
              bind:value={dispositionRaw}
              onkeydown={onCellKey}
              list="dl-disposition"
              placeholder="FLEC"
              class="cell-input uppercase text-pink-300"
            />
          </td>
          <td class="px-1 py-1">
            <input
              type="text"
              bind:value={notes}
              onkeydown={onCellKey}
              placeholder="optional"
              class="cell-input"
            />
          </td>
          <td class="px-1 py-0.5">
            <button
              type="button"
              onclick={submit}
              disabled={submitting}
              title="press Enter or click to add row"
              class="w-full rounded bg-emerald-500 hover:bg-emerald-400 active:bg-emerald-600 text-neutral-950 font-semibold px-2 py-1 disabled:opacity-50 transition"
            >
              {submitting ? '…' : '+ Add ↵'}
            </button>
          </td>
        </tr>

        <!-- HISTORICAL ROWS -->
        {#if rows.length === 0}
          <tr>
            <td colspan="15" class="px-3 py-4 text-center text-neutral-500 italic">
              no events yet — type into the green row above and press <kbd class="px-1 bg-neutral-800 rounded">Enter</kbd>
            </td>
          </tr>
        {:else}
          {#each rows as row (row.id)}
            <tr class="border-t border-neutral-900 hover:bg-neutral-900/40 text-neutral-300">
              <td></td>
              <td class="px-2 py-1 text-neutral-200">{fmtDate(row.recv_date)}</td>
              <td class="px-2 py-1 text-neutral-500">{fmtDate(row.prod_date)}</td>
              <td class="px-2 py-1 text-neutral-300">{row.batch}</td>
              <td class="px-2 py-1 text-neutral-300">{row.shift_code ?? ''}</td>
              <td class="px-2 py-1 text-violet-300">{row.grade_code}</td>
              <td class="px-2 py-1 text-cyan-300">{row.source_code}</td>
              <td class="px-2 py-1 text-neutral-500 italic">{row.plant_code ?? ''}</td>
              <td class="px-2 py-1 text-amber-300">{row.warehouse_code ?? ''}</td>
              <td class="px-2 py-1">{row.whse_side ?? ''}</td>
              <td class="px-2 py-1 text-right text-neutral-100 font-semibold">
                {fmtKg(row.weight_kg)}
              </td>
              <td class="px-2 py-1 text-right text-neutral-300">{row.flec_count ?? ''}</td>
              <td class="px-2 py-1">
                <span class={dispositionBadgeClass(row)}>{disp(row)}</span>
              </td>
              <td class="px-2 py-1 text-neutral-500 truncate" title={row.notes ?? ''}>
                {row.notes ?? ''}
              </td>
              <td></td>
            </tr>
          {/each}
        {/if}
      </tbody>
    </table>
  </div>

  <!-- Datalists (invisible). They feed the type-ahead inputs above. -->
  <datalist id="dl-shifts">
    {#each lookups.shifts as s}<option value={s.code}></option>{/each}
  </datalist>
  <datalist id="dl-grades">
    {#each lookups.grades as g}<option value={g.code}></option>{/each}
  </datalist>
  <datalist id="dl-sources">
    {#each lookups.source_locations as s}<option value={s.code}></option>{/each}
  </datalist>
  <datalist id="dl-plants">
    {#each lookups.plants as p}<option value={p.code}></option>{/each}
  </datalist>
  <datalist id="dl-warehouses">
    {#each lookups.warehouses as w}<option value={w.code}></option>{/each}
  </datalist>
  <datalist id="dl-sides">
    <option value="LS"></option><option value="RS"></option>
  </datalist>
  <datalist id="dl-disposition">
    {#each dispositionOptions as d}<option value={d}></option>{/each}
  </datalist>

  <div class="flex items-center justify-between px-3 py-1.5 border-t border-neutral-800 text-xs text-neutral-600 font-mono">
    <div class="flex items-center gap-3">
      <span><kbd class="px-1 bg-neutral-800 rounded">↵</kbd> add row</span>
      <span><kbd class="px-1 bg-neutral-800 rounded">esc</kbd> clear</span>
      <span><kbd class="px-1 bg-neutral-800 rounded">tab</kbd> next cell</span>
    </div>
    <button
      type="button"
      onclick={clearEditable}
      class="rounded border border-neutral-800 hover:border-neutral-700 px-2 py-0.5 text-neutral-400 hover:text-neutral-200"
    >
      Clear
    </button>
  </div>

  <!-- Bulk paste panel -->
  {#if bulkOpen}
    <div class="px-3 py-3 border-t border-neutral-800 space-y-2 bg-neutral-900/40">
      <div class="text-xs text-neutral-500 font-mono">
        Paste rows from Excel/Sheets (TSV) or comma-separated. Header row optional.
        Column order: RECV, PROD, BATCH, SH, GRD, SRC, PLT, WHSE, SD, WT, FLEC, DISP, NOTES.
      </div>
      <textarea
        bind:value={bulkText}
        rows="6"
        placeholder={`5/8\t5/8\tMAY\tM\t3X50\tTNK 1\tW6\tWHSE 7\tRS\t14000\t30\tFLEC\t\n5/8\t5/8\tMAY\tM\t3X50\tTNK 2\tW6\tWHSE 7\tRS\t12500\t28\tFLEC\t`}
        class="w-full rounded bg-neutral-950 border border-neutral-800 p-2 text-xs font-mono"
      ></textarea>
      {#if bulkErr}
        <pre class="text-xs text-red-300 whitespace-pre-wrap font-mono">{bulkErr}</pre>
      {/if}
      <div class="flex items-center gap-2">
        <button
          type="button"
          onclick={submitBulk}
          disabled={bulkBusy}
          class="rounded bg-emerald-700 hover:bg-emerald-600 px-3 py-1 text-xs font-medium disabled:opacity-50"
        >
          {bulkBusy ? 'inserting…' : 'Submit all'}
        </button>
        <span class="text-xs text-neutral-600 font-mono">
          all rows insert in one transaction; if any fails, none land
        </span>
      </div>
    </div>
  {/if}
</div>

<style>
  /* Excel-feel: borderless inputs that fill their cell. Subtle focus ring. */
  :global(.cell-input) {
    width: 100%;
    background: transparent;
    border: 1px solid transparent;
    color: rgb(229 229 229);
    padding: 0.25rem 0.5rem;
    font-family: inherit;
    font-size: inherit;
    border-radius: 2px;
    outline: none;
  }
  :global(.cell-input:focus) {
    background: rgb(10 10 10);
    border-color: rgb(34 197 94 / 0.5);
  }
  :global(.cell-input::placeholder) {
    color: rgb(82 82 82);
  }
  /* Hide the date input's native picker icon — too visually noisy for a table cell.
     The cell still opens the picker on click. */
  :global(.cell-input[type='date']::-webkit-calendar-picker-indicator) {
    opacity: 0.3;
    cursor: pointer;
  }
</style>

