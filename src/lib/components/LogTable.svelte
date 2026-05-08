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
  /** Defensive trim: Svelte's bind:value on <input type="number"> hands us a
   *  Number, not a String, and `.trim()` on a Number throws. */
  export function trimStr(v: unknown): string {
    if (v === undefined || v === null) return '';
    return String(v).trim();
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
  // Drafts: editable rows the operator stages BEFORE pressing Submit all.
  // After successful submit, drafts collapse back to one empty row.
  // After a partial failure, successful drafts are removed; the failed one
  // gets an inline error and stays so the operator can fix and retry.
  // -------------------------------------------------------------------------

  type Draft = {
    id: string;
    recvDate: string;
    prodDate: string;
    batch: string;
    shiftCode: string;
    gradeCode: string;
    sourceCode: string;
    plantCodeOverride: string;
    warehouseCode: string;
    whseSide: string;
    weightStr: string | number;
    flecStr: string | number;
    dispositionRaw: string;
    notes: string;
    rowError: string | null;
  };

  function todayIso(): string {
    return new Date().toISOString().slice(0, 10);
  }
  function currentMonthName(): string {
    return new Date().toLocaleString('en-US', { month: 'long' }).toUpperCase();
  }
  function newDraftId(): string {
    return Math.random().toString(36).slice(2, 10);
  }

  /** Fresh draft seeded from `template`'s sticky fields (or codo defaults if
   *  `template` is null). The fields that change every row — weight, flec,
   *  notes — are NEVER copied. */
  function newDraft(template?: Draft | null): Draft {
    return {
      id: newDraftId(),
      recvDate: template?.recvDate ?? todayIso(),
      prodDate: template?.prodDate ?? todayIso(),
      batch: template?.batch ?? currentMonthName(),
      shiftCode: template?.shiftCode ?? 'M',
      gradeCode: template?.gradeCode ?? '',
      sourceCode: template?.sourceCode ?? '',
      plantCodeOverride: template?.plantCodeOverride ?? '',
      warehouseCode: template?.warehouseCode ?? '',
      whseSide: template?.whseSide ?? '',
      weightStr: '',
      flecStr: '',
      dispositionRaw: template?.dispositionRaw ?? 'FLEC',
      notes: '',
      rowError: null
    };
  }

  let drafts: Draft[] = $state([newDraft()]);
  let submitting = $state(false);
  let topErr = $state<string | null>(null);

  function addRow() {
    const last = drafts[drafts.length - 1] ?? null;
    drafts = [...drafts, newDraft(last)];
  }

  function removeRow(id: string) {
    if (drafts.length <= 1) {
      drafts = [newDraft()];
    } else {
      drafts = drafts.filter((d) => d.id !== id);
    }
  }

  function clearAllDrafts() {
    drafts = [newDraft()];
    topErr = null;
  }

  // -------------------------------------------------------------------------
  // Per-draft plant derivation per §7.2 — non-FLEC sources force the plant.
  // -------------------------------------------------------------------------

  const sourceByCode = $derived.by(() => {
    const map = new Map<string, SourceLocationRow>();
    for (const s of lookups.source_locations) map.set(s.code, s);
    return map;
  });

  function selectedSourceFor(d: Draft): SourceLocationRow | null {
    return sourceByCode.get(trimStr(d.sourceCode).toUpperCase()) ?? null;
  }
  function sourceForcesPlant(d: Draft): boolean {
    const s = selectedSourceFor(d);
    return s !== null && s.kind !== 'warehouse_flec';
  }
  function forcedPlantCode(d: Draft): string {
    const s = selectedSourceFor(d);
    if (!s || s.kind === 'warehouse_flec' || s.plant_id == null) return '';
    return lookups.plants.find((p) => p.id === s.plant_id)?.code ?? '';
  }
  function displayPlant(d: Draft): string {
    return sourceForcesPlant(d) ? forcedPlantCode(d) : d.plantCodeOverride;
  }

  // -------------------------------------------------------------------------
  // Validation + payload building.
  // -------------------------------------------------------------------------

  function validateDraft(d: Draft): string | null {
    const required = ['batch', 'gradeCode', 'sourceCode', 'weightStr'] as const;
    const missing: string[] = [];
    if (!trimStr(d.batch)) missing.push('batch');
    if (!trimStr(d.gradeCode)) missing.push('grade');
    if (!trimStr(d.sourceCode)) missing.push('source');
    if (!trimStr(d.weightStr)) missing.push('weight');
    if (missing.length > 0) return `missing ${missing.join(', ')}`;

    const wt = parseFloat(trimStr(d.weightStr));
    if (!(wt > 0)) return 'weight must be > 0';

    const flecRaw = trimStr(d.flecStr);
    if (flecRaw !== '') {
      const n = parseInt(flecRaw, 10);
      if (!Number.isFinite(n) || n <= 0) return 'flec must be positive integer';
    }
    return null;
  }

  function toInput(d: Draft): CreateProductionEventInput {
    const wt = parseFloat(trimStr(d.weightStr));
    const flecRaw = trimStr(d.flecStr);
    const flecN = flecRaw === '' ? null : parseInt(flecRaw, 10);
    const sel = selectedSourceFor(d);
    return {
      recvDate: d.recvDate,
      prodDate: trimStr(d.prodDate) === '' ? null : d.prodDate,
      batch: trimStr(d.batch).toUpperCase(),
      shiftCode: trimStr(d.shiftCode) === '' ? null : trimStr(d.shiftCode).toUpperCase(),
      gradeCode: trimStr(d.gradeCode).toUpperCase(),
      sourceCode: trimStr(d.sourceCode).toUpperCase(),
      plantCodeOverride:
        sel?.kind === 'warehouse_flec' && trimStr(d.plantCodeOverride)
          ? trimStr(d.plantCodeOverride).toUpperCase()
          : null,
      warehouseCode:
        trimStr(d.warehouseCode) === '' ? null : trimStr(d.warehouseCode).toUpperCase(),
      dispositionRaw: trimStr(d.dispositionRaw).toUpperCase(),
      weightKg: wt,
      flecCount: flecN,
      whseSide: trimStr(d.whseSide) === '' ? null : trimStr(d.whseSide).toUpperCase(),
      flecStat: null,
      dvoBatchId: null,
      notes: trimStr(d.notes) === '' ? null : String(d.notes)
    };
  }

  // -------------------------------------------------------------------------
  // Submit all — one transaction via create_production_events_bulk.
  // -------------------------------------------------------------------------

  async function submitAll() {
    topErr = null;
    if (submitting) return;
    if (drafts.length === 0) return;

    submitting = true;
    try {
      // Client-side validation pass first — surface every bad row at once.
      let anyError = false;
      drafts = drafts.map((d) => {
        const e = validateDraft(d);
        if (e) anyError = true;
        return { ...d, rowError: e };
      });
      if (anyError) {
        topErr = 'fix the highlighted row(s) before submitting';
        return;
      }

      const inputs = drafts.map(toInput);
      const inserted = await invoke<ProductionEventRow[]>(
        'create_production_events_bulk',
        { inputs }
      );
      onSaved(inserted);
      // Reset to a single fresh draft templated from the last submitted.
      const lastSubmitted = drafts[drafts.length - 1];
      drafts = [newDraft(lastSubmitted)];
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      topErr = msg;
      // Try to parse "row N of M failed (X succeeded before): ..." and
      // surface the failure on that draft. Successful prefix drafts get
      // dropped; failed one stays for editing; rest stay queued.
      const m = msg.match(/row (\d+) of \d+ failed \((\d+) succeeded before\): (.+)/);
      if (m) {
        const failedIdx = parseInt(m[1], 10) - 1;
        const succeeded = parseInt(m[2], 10);
        const detail = m[3];
        // Remove successful prefix; mark failed; keep rest.
        drafts = drafts.slice(succeeded).map((d, i) => {
          if (i === failedIdx - succeeded) return { ...d, rowError: detail };
          return d;
        });
      }
    } finally {
      submitting = false;
    }
  }

  function onCellKey(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      submitAll();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      submitAll();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      clearAllDrafts();
    }
  }

  // -------------------------------------------------------------------------
  // Bulk paste from Excel/Sheets — unchanged from the previous Phase B.
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
        bulkErr = `line ${i + (looksLikeHeader ? 2 : 1)}: weight must be > 0`;
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
      const inserted = await invoke<ProductionEventRow[]>(
        'create_production_events_bulk',
        { inputs }
      );
      onSaved(inserted);
      bulkOpen = false;
      bulkText = '';
    } catch (e) {
      bulkErr = e instanceof Error ? e.message : String(e);
    } finally {
      bulkBusy = false;
    }
  }

  function splitCells(line: string): string[] {
    if (line.includes('\t')) return line.split('\t').map((c) => c.trim());
    return line.split(',').map((c) => c.trim());
  }
  function normalizeDate(s: string): string | null {
    const trimmed = s.trim();
    if (!trimmed) return null;
    if (/^\d{4}-\d{2}-\d{2}$/.test(trimmed)) return trimmed;
    const m = trimmed.match(/^(\d{1,2})\/(\d{1,2})(?:\/(\d{2,4}))?$/);
    if (m) {
      const month = m[1].padStart(2, '0');
      const day = m[2].padStart(2, '0');
      let year = m[3];
      if (!year) year = new Date().getFullYear().toString();
      else if (year.length === 2) year = '20' + year;
      return `${year}-${month}-${day}`;
    }
    return trimmed;
  }
</script>

<div class="rounded-md border border-neutral-800 bg-neutral-950 overflow-hidden">
  <div class="flex items-baseline justify-between px-3 py-1.5 border-b border-neutral-800">
    <h3 class="text-xs uppercase tracking-wide text-neutral-500">Production log</h3>
    <div class="flex items-center gap-3 text-xs text-neutral-600 font-mono">
      <span>history: {rows.length}</span>
      <span>·</span>
      <span class="text-emerald-400">drafts: {drafts.length}</span>
      <button
        type="button"
        class="rounded border border-neutral-800 hover:border-neutral-700 px-2 py-0.5 text-neutral-400 hover:text-neutral-200"
        onclick={() => (bulkOpen = !bulkOpen)}
      >
        {bulkOpen ? 'close bulk' : 'bulk paste'}
      </button>
    </div>
  </div>

  {#if topErr}
    <div class="px-3 py-2 text-xs text-red-100 bg-red-900/60 border-b border-red-700 font-mono">
      ⚠ {topErr}
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
        <col style="width: 56px" /><!-- SIDE -->
        <col style="width: 90px" /><!-- WEIGHT -->
        <col style="width: 60px" /><!-- FLEC -->
        <col style="width: 70px" /><!-- DEST -->
        <col /><!-- NOTES (flex) -->
        <col style="width: 36px" /><!-- × button -->
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
        <!-- DRAFT ROWS (editable). The operator can stage many. -->
        {#each drafts as draft, di (draft.id)}
          <tr
            class="bg-emerald-950/30 border-y border-emerald-700/30
                   {draft.rowError ? 'ring-1 ring-red-500/60 bg-red-950/30' : ''}"
          >
            <td class="px-1 text-center text-emerald-400 font-bold" title="draft row">
              +
            </td>
            <td class="px-1 py-1">
              <input
                type="date"
                bind:value={draft.recvDate}
                onkeydown={onCellKey}
                class="cell-input"
              />
            </td>
            <td class="px-1 py-1">
              <input
                type="date"
                bind:value={draft.prodDate}
                onkeydown={onCellKey}
                class="cell-input"
              />
            </td>
            <td class="px-1 py-1">
              <input
                type="text"
                bind:value={draft.batch}
                onkeydown={onCellKey}
                placeholder="MAY"
                class="cell-input uppercase text-neutral-100"
              />
            </td>
            <td class="px-1 py-1">
              <input
                type="text"
                bind:value={draft.shiftCode}
                onkeydown={onCellKey}
                list="dl-shifts"
                placeholder="M"
                class="cell-input uppercase"
              />
            </td>
            <td class="px-1 py-1">
              <input
                type="text"
                bind:value={draft.gradeCode}
                onkeydown={onCellKey}
                list="dl-grades"
                placeholder="3X50"
                class="cell-input uppercase text-violet-300"
              />
            </td>
            <td class="px-1 py-1">
              <input
                type="text"
                bind:value={draft.sourceCode}
                onkeydown={onCellKey}
                list="dl-sources"
                placeholder="TNK 1"
                class="cell-input uppercase text-cyan-300"
              />
            </td>
            <td class="px-1 py-1">
              {#if sourceForcesPlant(draft)}
                <input
                  type="text"
                  value={forcedPlantCode(draft)}
                  readonly
                  tabindex="-1"
                  title="auto from source"
                  class="cell-input text-neutral-500 cursor-default italic"
                />
              {:else}
                <input
                  type="text"
                  bind:value={draft.plantCodeOverride}
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
                bind:value={draft.warehouseCode}
                onkeydown={onCellKey}
                list="dl-warehouses"
                placeholder="—"
                class="cell-input uppercase text-amber-300"
              />
            </td>
            <td class="px-1 py-1">
              <input
                type="text"
                bind:value={draft.whseSide}
                onkeydown={onCellKey}
                list="dl-sides"
                placeholder="—"
                class="cell-input uppercase"
              />
            </td>
            <td class="px-1 py-1">
              <input
                type="number"
                bind:value={draft.weightStr}
                onkeydown={onCellKey}
                step="0.01"
                min="0"
                placeholder="0"
                class="cell-input text-right text-neutral-50 font-semibold"
              />
            </td>
            <td class="px-1 py-1">
              <input
                type="number"
                bind:value={draft.flecStr}
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
                bind:value={draft.dispositionRaw}
                onkeydown={onCellKey}
                list="dl-disposition"
                placeholder="FLEC"
                class="cell-input uppercase text-pink-300"
              />
            </td>
            <td class="px-1 py-1">
              <input
                type="text"
                bind:value={draft.notes}
                onkeydown={onCellKey}
                placeholder="optional"
                class="cell-input"
              />
            </td>
            <td class="px-1 text-center">
              <button
                type="button"
                onclick={() => removeRow(draft.id)}
                title="remove this draft row"
                class="text-neutral-500 hover:text-red-400 text-base leading-none"
              >×</button>
            </td>
          </tr>
          {#if draft.rowError}
            <tr class="bg-red-950/40">
              <td></td>
              <td colspan="14" class="px-3 py-1 text-[11px] text-red-300 font-mono">
                ⚠ row {di + 1}: {draft.rowError}
              </td>
            </tr>
          {/if}
        {/each}

        <!-- DRAFTS ACTION ROW: + Add row | Submit all | Clear -->
        <tr class="bg-neutral-900/70 border-y border-neutral-800">
          <td colspan="15" class="px-3 py-2">
            <div class="flex items-center gap-2">
              <button
                type="button"
                onclick={addRow}
                class="rounded border border-emerald-700/40 hover:border-emerald-500 px-2 py-1 text-xs text-emerald-300 hover:text-emerald-100 font-medium"
              >
                + Add row
              </button>
              <button
                type="button"
                onclick={clearAllDrafts}
                class="rounded border border-neutral-800 hover:border-neutral-700 px-2 py-1 text-xs text-neutral-400 hover:text-neutral-200"
              >
                Clear drafts
              </button>
              <span class="text-xs text-neutral-600 font-mono">
                <kbd class="px-1 bg-neutral-800 rounded">↵</kbd> submit all
                · <kbd class="px-1 bg-neutral-800 rounded">esc</kbd> clear
                · <kbd class="px-1 bg-neutral-800 rounded">tab</kbd> next cell
              </span>
              <div class="flex-1"></div>
              <button
                type="button"
                onclick={submitAll}
                disabled={submitting}
                class="rounded bg-emerald-500 hover:bg-emerald-400 active:bg-emerald-600 text-neutral-950 font-semibold px-4 py-1.5 text-xs disabled:opacity-50 transition"
              >
                {submitting ? 'saving…' : `Submit all (${drafts.length})`}
              </button>
            </div>
          </td>
        </tr>

        <!-- HISTORY: saved, immutable rows below the drafts. -->
        <tr class="bg-neutral-900 text-neutral-500 text-[10px] uppercase tracking-wider">
          <td colspan="15" class="px-3 py-1">
            ── History (last {rows.length}) ──
          </td>
        </tr>
        {#if rows.length === 0}
          <tr>
            <td colspan="15" class="px-3 py-4 text-center text-neutral-600 italic">
              no saved events yet
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

  <!-- Datalists (invisible). They feed the type-ahead inputs. -->
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
    <option value="FLEC"></option>
    <option value="C1"></option><option value="C2"></option>
    <option value="C3"></option><option value="C4"></option>
    <option value="RK1"></option><option value="RK2"></option>
    <option value="RK3"></option><option value="RK4"></option>
  </datalist>

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
        placeholder={'5/8\t5/8\tMAY\tM\t3X50\tTNK 1\tW6\tWHSE 7\tRS\t14000\t30\tFLEC\t\n5/8\t5/8\tMAY\tM\t3X50\tTNK 2\tW6\tWHSE 7\tRS\t12500\t28\tFLEC\t'}
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
          all rows insert in one transaction
        </span>
      </div>
    </div>
  {/if}
</div>

<style>
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
  :global(.cell-input[type='date']::-webkit-calendar-picker-indicator) {
    opacity: 0.3;
    cursor: pointer;
  }
</style>
