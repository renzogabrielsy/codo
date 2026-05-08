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
  /** Disposition pill colour by kind — emerald FLEC / sky Crusher / amber Kiln.
   *  Tuned for both light + dark modes. */
  export function dispositionBadgeClass(row: { disposition_kind: string }): string {
    const base =
      'inline-block rounded px-1.5 py-0.5 text-[11px] font-bold tracking-wider border';
    switch (row.disposition_kind) {
      case 'flec_bagging':
        return `${base} bg-emerald-100 text-emerald-800 border-emerald-300 dark:bg-emerald-700/40 dark:text-emerald-200 dark:border-emerald-600/40`;
      case 'partner_crusher':
        return `${base} bg-sky-100 text-sky-800 border-sky-300 dark:bg-sky-700/40 dark:text-sky-200 dark:border-sky-600/40`;
      case 'partner_kiln':
        return `${base} bg-amber-100 text-amber-800 border-amber-300 dark:bg-amber-700/40 dark:text-amber-200 dark:border-amber-600/40`;
      default:
        return `${base} bg-neutral-100 text-neutral-800 dark:bg-neutral-700/40 dark:text-neutral-200`;
    }
  }
  /** Defensive trim: Svelte's bind:value on <input type="number"> hands us a
   *  Number, not a String, and `.trim()` on a Number throws. */
  export function trimStr(v: unknown): string {
    if (v === undefined || v === null) return '';
    return String(v).trim();
  }

  // -------------------------------------------------------------------------
  // Row tinting by direction-of-flow.
  //   FLEC  → "we add to inventory"   → emerald (green = in)
  //   C1-C4 / RK1-RK4 → "CCC takes from us" → purple (out)
  // Applied to both drafts (reactive on dispositionRaw) and history rows
  // (on disposition_kind).
  // -------------------------------------------------------------------------

  export function rowTintByKind(kind: string): string {
    switch (kind) {
      case 'flec_bagging':
        return 'bg-emerald-50/80 dark:bg-emerald-950/30';
      case 'partner_crusher':
      case 'partner_kiln':
        return 'bg-purple-50/80 dark:bg-purple-950/30';
      default:
        return '';
    }
  }
  export function rowTintByRaw(raw: unknown): string {
    const u = trimStr(raw).toUpperCase();
    if (u === 'FLEC') return 'bg-emerald-100 dark:bg-emerald-950/30';
    if (/^(C[1-4]|RK[1-4])$/.test(u)) return 'bg-purple-100 dark:bg-purple-950/30';
    return '';
  }
  export function draftBorderByRaw(raw: unknown): string {
    const u = trimStr(raw).toUpperCase();
    if (u === 'FLEC') return 'border-emerald-400 dark:border-emerald-700/40';
    if (/^(C[1-4]|RK[1-4])$/.test(u)) return 'border-purple-400 dark:border-purple-700/40';
    return 'border-neutral-300 dark:border-neutral-700/40';
  }

  // -------------------------------------------------------------------------
  // Date parsing — accept loose formats so the operator can just type "5/8".
  // -------------------------------------------------------------------------

  /** Parse '5/8' / '5/8/26' / '5/8/2026' / '2026-05-08' → ISO 'YYYY-MM-DD'.
   *  Returns null if unparseable so the Rust side rejects with a clear error. */
  export function parseDateLoose(raw: unknown): string | null {
    const s = trimStr(raw);
    if (!s) return null;
    if (/^\d{4}-\d{2}-\d{2}$/.test(s)) return s;
    const m = s.match(/^(\d{1,2})\/(\d{1,2})(?:\/(\d{2,4}))?$/);
    if (!m) return null;
    const month = m[1].padStart(2, '0');
    const day = m[2].padStart(2, '0');
    let year = m[3];
    if (!year) year = new Date().getFullYear().toString();
    else if (year.length === 2) year = '20' + year;
    return `${year}-${month}-${day}`;
  }

  /** Today as 'M/D' for compact, Excel-like default in the input cells. */
  export function todayShort(): string {
    const d = new Date();
    return `${d.getMonth() + 1}/${d.getDate()}`;
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

  function currentMonthName(): string {
    return new Date().toLocaleString('en-US', { month: 'long' }).toUpperCase();
  }
  function newDraftId(): string {
    return Math.random().toString(36).slice(2, 10);
  }

  /** Fresh draft seeded from `template`'s sticky fields (or codo defaults if
   *  `template` is null). The fields that change every row — weight, flec,
   *  notes — are NEVER copied. Dates default to today as `M/D` (operator can
   *  type `5/8/26` for cross-year work). */
  function newDraft(template?: Draft | null): Draft {
    return {
      id: newDraftId(),
      recvDate: template?.recvDate ?? todayShort(),
      prodDate: template?.prodDate ?? todayShort(),
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
    const missing: string[] = [];
    if (!trimStr(d.batch)) missing.push('batch');
    if (!trimStr(d.gradeCode)) missing.push('grade');
    if (!trimStr(d.sourceCode)) missing.push('source');
    if (!trimStr(d.weightStr)) missing.push('weight');
    if (missing.length > 0) return `missing ${missing.join(', ')}`;

    if (!parseDateLoose(d.recvDate)) {
      return `recv date '${trimStr(d.recvDate)}' — try M/D or M/D/YY`;
    }
    if (trimStr(d.prodDate) !== '' && !parseDateLoose(d.prodDate)) {
      return `prod date '${trimStr(d.prodDate)}' — try M/D or M/D/YY`;
    }

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
    const recvIso = parseDateLoose(d.recvDate) ?? '';
    const prodIso = parseDateLoose(d.prodDate); // null if blank or unparseable
    return {
      recvDate: recvIso,
      prodDate: prodIso,
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

<div class="rounded-md border border-neutral-300 bg-white dark:border-neutral-800 dark:bg-neutral-950 overflow-hidden">
  <div class="flex items-baseline justify-between px-3 py-1.5 border-b border-neutral-300 dark:border-neutral-800">
    <h3 class="text-xs uppercase tracking-wide font-semibold text-neutral-700 dark:text-neutral-400">
      Production log
    </h3>
    <div class="flex items-center gap-3 text-xs font-mono text-neutral-600 dark:text-neutral-500">
      <span>history: {rows.length}</span>
      <span class="text-neutral-400 dark:text-neutral-700">·</span>
      <span class="text-emerald-700 dark:text-emerald-400">drafts: {drafts.length}</span>
      <button
        type="button"
        class="rounded border border-neutral-300 hover:border-neutral-500 dark:border-neutral-700 dark:hover:border-neutral-500 px-2 py-0.5 text-neutral-700 hover:bg-neutral-100 dark:text-neutral-300 dark:hover:bg-neutral-800"
        onclick={() => (bulkOpen = !bulkOpen)}
      >
        {bulkOpen ? 'close bulk' : 'bulk paste'}
      </button>
    </div>
  </div>

  {#if topErr}
    <div class="px-3 py-2 text-sm font-mono font-medium text-red-800 bg-red-100 border-b border-red-300 dark:text-red-100 dark:bg-red-900/60 dark:border-red-700">
      ⚠ {topErr}
    </div>
  {/if}

  <div class="overflow-x-auto">
    <table class="w-full text-sm font-mono">
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
      <thead class="bg-neutral-100 text-neutral-700 dark:bg-neutral-900 dark:text-neutral-300 text-[12px] uppercase tracking-wider">
        <tr>
          <th></th>
          <th class="px-2 py-2 text-left font-semibold">Recv</th>
          <th class="px-2 py-2 text-left font-semibold">Prod</th>
          <th class="px-2 py-2 text-left font-semibold">Batch</th>
          <th class="px-2 py-2 text-left font-semibold">Shift</th>
          <th class="px-2 py-2 text-left font-semibold">Grade</th>
          <th class="px-2 py-2 text-left font-semibold">Source</th>
          <th class="px-2 py-2 text-left font-semibold">Plant</th>
          <th class="px-2 py-2 text-left font-semibold">Warehouse</th>
          <th class="px-2 py-2 text-left font-semibold">Side</th>
          <th class="px-2 py-2 text-right font-semibold">Weight kg</th>
          <th class="px-2 py-2 text-right font-semibold">Flec</th>
          <th class="px-2 py-2 text-left font-semibold">Dest</th>
          <th class="px-2 py-2 text-left font-semibold">Notes</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        <!-- DRAFT ROWS (editable). The operator can stage many. -->
        {#each drafts as draft, di (draft.id)}
          {@const tint = draft.rowError
            ? 'bg-red-100 dark:bg-red-950/40'
            : rowTintByRaw(draft.dispositionRaw)}
          {@const border = draft.rowError
            ? 'border-red-500'
            : draftBorderByRaw(draft.dispositionRaw)}
          <tr class="border-y-2 {border} {tint}">
            <td
              class="px-1 text-center font-bold {draft.rowError
                ? 'text-red-600 dark:text-red-400'
                : 'text-emerald-700 dark:text-emerald-400'}"
              title="draft row"
            >
              +
            </td>
            <td class="px-1 py-1">
              <input
                type="text"
                bind:value={draft.recvDate}
                onkeydown={onCellKey}
                placeholder="M/D"
                class="cell-input font-mono"
              />
            </td>
            <td class="px-1 py-1">
              <input
                type="text"
                bind:value={draft.prodDate}
                onkeydown={onCellKey}
                placeholder="M/D"
                class="cell-input font-mono"
              />
            </td>
            <td class="px-1 py-1">
              <input
                type="text"
                bind:value={draft.batch}
                onkeydown={onCellKey}
                placeholder="MAY"
                class="cell-input uppercase font-semibold text-neutral-900 dark:text-neutral-100"
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
                class="cell-input uppercase text-violet-700 dark:text-violet-300"
              />
            </td>
            <td class="px-1 py-1">
              <input
                type="text"
                bind:value={draft.sourceCode}
                onkeydown={onCellKey}
                list="dl-sources"
                placeholder="TNK 1"
                class="cell-input uppercase text-cyan-700 dark:text-cyan-300"
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
                  class="cell-input italic cursor-default text-neutral-500 dark:text-neutral-500"
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
                class="cell-input uppercase text-amber-700 dark:text-amber-300"
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
                class="cell-input text-right font-semibold text-neutral-900 dark:text-neutral-50"
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
                class="cell-input uppercase text-pink-700 dark:text-pink-300"
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
                class="text-base leading-none text-neutral-500 hover:text-red-600 dark:hover:text-red-400"
              >×</button>
            </td>
          </tr>
          {#if draft.rowError}
            <tr class="bg-red-100 dark:bg-red-950/40">
              <td></td>
              <td
                colspan="14"
                class="px-3 py-1 text-[12px] font-mono text-red-700 dark:text-red-300"
              >
                ⚠ row {di + 1}: {draft.rowError}
              </td>
            </tr>
          {/if}
        {/each}

        <!-- DRAFTS ACTION ROW: + Add row | Submit all | Clear -->
        <tr class="bg-neutral-100 dark:bg-neutral-900/70 border-y border-neutral-300 dark:border-neutral-800">
          <td colspan="15" class="px-3 py-2">
            <div class="flex items-center gap-2">
              <button
                type="button"
                onclick={addRow}
                class="rounded border border-emerald-500/60 hover:border-emerald-600 dark:border-emerald-700/40 dark:hover:border-emerald-500 px-2 py-1 text-sm font-medium text-emerald-800 hover:bg-emerald-50 dark:text-emerald-300 dark:hover:bg-emerald-950/40 dark:hover:text-emerald-100"
              >
                + Add row
              </button>
              <button
                type="button"
                onclick={clearAllDrafts}
                class="rounded border border-neutral-300 hover:border-neutral-500 dark:border-neutral-700 dark:hover:border-neutral-500 px-2 py-1 text-sm text-neutral-700 hover:bg-neutral-50 dark:text-neutral-400 dark:hover:bg-neutral-800 dark:hover:text-neutral-200"
              >
                Clear drafts
              </button>
              <span class="text-xs font-mono text-neutral-500 dark:text-neutral-600">
                <kbd class="px-1 rounded bg-neutral-200 text-neutral-800 dark:bg-neutral-800 dark:text-neutral-200">↵</kbd> submit all
                · <kbd class="px-1 rounded bg-neutral-200 text-neutral-800 dark:bg-neutral-800 dark:text-neutral-200">esc</kbd> clear
                · <kbd class="px-1 rounded bg-neutral-200 text-neutral-800 dark:bg-neutral-800 dark:text-neutral-200">tab</kbd> next cell
              </span>
              <div class="flex-1"></div>
              <button
                type="button"
                onclick={submitAll}
                disabled={submitting}
                class="rounded bg-emerald-600 hover:bg-emerald-500 active:bg-emerald-700 dark:bg-emerald-500 dark:hover:bg-emerald-400 dark:active:bg-emerald-600 text-white dark:text-neutral-950 font-semibold px-4 py-1.5 text-sm disabled:opacity-50 transition"
              >
                {submitting ? 'saving…' : `Submit all (${drafts.length})`}
              </button>
            </div>
          </td>
        </tr>

        <!-- HISTORY: saved, immutable rows below the drafts. -->
        <tr class="bg-neutral-100 text-neutral-600 dark:bg-neutral-900 dark:text-neutral-500 text-[11px] uppercase tracking-wider font-semibold">
          <td colspan="15" class="px-3 py-1.5">
            ── History (last {rows.length}) ──
          </td>
        </tr>
        {#if rows.length === 0}
          <tr>
            <td colspan="15" class="px-3 py-4 text-center italic text-neutral-500 dark:text-neutral-600">
              no saved events yet
            </td>
          </tr>
        {:else}
          {#each rows as row (row.id)}
            <tr
              class="border-t border-neutral-200 hover:bg-neutral-50 dark:border-neutral-900 dark:hover:bg-neutral-900/40 text-neutral-800 dark:text-neutral-300 {rowTintByKind(
                row.disposition_kind
              )}"
            >
              <td></td>
              <td class="px-2 py-1.5 text-neutral-900 dark:text-neutral-200">
                {fmtDate(row.recv_date)}
              </td>
              <td class="px-2 py-1.5 text-neutral-500">{fmtDate(row.prod_date)}</td>
              <td class="px-2 py-1.5 text-neutral-800 dark:text-neutral-300">{row.batch}</td>
              <td class="px-2 py-1.5 text-neutral-800 dark:text-neutral-300">
                {row.shift_code ?? ''}
              </td>
              <td class="px-2 py-1.5 text-violet-700 dark:text-violet-300">{row.grade_code}</td>
              <td class="px-2 py-1.5 text-cyan-700 dark:text-cyan-300">{row.source_code}</td>
              <td class="px-2 py-1.5 italic text-neutral-500">{row.plant_code ?? ''}</td>
              <td class="px-2 py-1.5 text-amber-700 dark:text-amber-300">
                {row.warehouse_code ?? ''}
              </td>
              <td class="px-2 py-1.5">{row.whse_side ?? ''}</td>
              <td class="px-2 py-1.5 text-right font-semibold text-neutral-900 dark:text-neutral-100">
                {fmtKg(row.weight_kg)}
              </td>
              <td class="px-2 py-1.5 text-right text-neutral-700 dark:text-neutral-300">
                {row.flec_count ?? ''}
              </td>
              <td class="px-2 py-1.5">
                <span class={dispositionBadgeClass(row)}>{disp(row)}</span>
              </td>
              <td class="px-2 py-1.5 truncate text-neutral-600 dark:text-neutral-500" title={row.notes ?? ''}>
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
    <div class="px-3 py-3 space-y-2 border-t border-neutral-300 bg-neutral-50 dark:border-neutral-800 dark:bg-neutral-900/40">
      <div class="text-xs font-mono text-neutral-700 dark:text-neutral-500">
        Paste rows from Excel/Sheets (TSV) or comma-separated. Header row optional.
        Column order: RECV, PROD, BATCH, SH, GRD, SRC, PLT, WHSE, SD, WT, FLEC, DISP, NOTES.
      </div>
      <textarea
        bind:value={bulkText}
        rows="6"
        placeholder={'5/8\t5/8\tMAY\tM\t3X50\tTNK 1\tW6\tWHSE 7\tRS\t14000\t30\tFLEC\t\n5/8\t5/8\tMAY\tM\t3X50\tTNK 2\tW6\tWHSE 7\tRS\t12500\t28\tFLEC\t'}
        class="w-full rounded border border-neutral-300 bg-white p-2 text-sm font-mono text-neutral-900 dark:border-neutral-800 dark:bg-neutral-950 dark:text-neutral-100"
      ></textarea>
      {#if bulkErr}
        <pre class="text-sm font-mono text-red-700 whitespace-pre-wrap dark:text-red-300">{bulkErr}</pre>
      {/if}
      <div class="flex items-center gap-2">
        <button
          type="button"
          onclick={submitBulk}
          disabled={bulkBusy}
          class="rounded bg-emerald-600 hover:bg-emerald-500 dark:bg-emerald-700 dark:hover:bg-emerald-600 px-3 py-1 text-sm font-medium text-white disabled:opacity-50"
        >
          {bulkBusy ? 'inserting…' : 'Submit all'}
        </button>
        <span class="text-xs font-mono text-neutral-600 dark:text-neutral-600">
          all rows insert in one transaction
        </span>
      </div>
    </div>
  {/if}
</div>

<style>
  /* Cell-shaped input that fills its td and blends into the table. Both
     light + dark colors so the same component reads in either mode. */
  :global(.cell-input) {
    width: 100%;
    background: transparent;
    border: 1px solid transparent;
    color: rgb(23 23 23);
    padding: 0.3rem 0.5rem;
    font-family: inherit;
    font-size: inherit;
    border-radius: 3px;
    outline: none;
  }
  :global(.dark .cell-input) {
    color: rgb(229 229 229);
  }
  :global(.cell-input:focus) {
    background: rgb(255 255 255);
    border-color: rgb(16 185 129);
    box-shadow: 0 0 0 2px rgb(16 185 129 / 0.2);
  }
  :global(.dark .cell-input:focus) {
    background: rgb(10 10 10);
    border-color: rgb(34 197 94 / 0.6);
    box-shadow: 0 0 0 2px rgb(34 197 94 / 0.15);
  }
  :global(.cell-input::placeholder) {
    color: rgb(163 163 163);
  }
  :global(.dark .cell-input::placeholder) {
    color: rgb(82 82 82);
  }
  :global(.cell-input[type='date']::-webkit-calendar-picker-indicator) {
    opacity: 0.3;
    cursor: pointer;
  }
</style>
