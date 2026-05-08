<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import type {
    LookupBundle,
    CreateProductionEventInput,
    ProductionEventRow,
    SourceLocationRow
  } from '$lib/types/codo';

  type Props = {
    lookups: LookupBundle;
    onSaved: (row: ProductionEventRow) => void;
  };
  let { lookups, onSaved }: Props = $props();

  // -------------------------------------------------------------------------
  // Form state. Strings throughout; the Rust side canonicalizes everything.
  // -------------------------------------------------------------------------

  function todayIso(): string {
    return new Date().toISOString().slice(0, 10);
  }

  let recvDate = $state(todayIso());
  let prodDate = $state(todayIso());
  let batch = $state('');               // sticky candidate (Phase B)
  let shiftCode = $state('M');          // sticky candidate (Phase B)
  let gradeCode = $state('');
  let sourceCode = $state('');
  let plantCodeOverride = $state('');
  let warehouseCode = $state('');
  let dispositionKind = $state<'flec_bagging' | 'partner_crusher' | 'partner_kiln'>(
    'flec_bagging'
  );
  let partnerEquipmentCode = $state('');
  let weightKg = $state('');
  let flecCount = $state('');
  let whseSide = $state('');
  let notes = $state('');

  let submitting = $state(false);
  let err = $state<string | null>(null);

  // -------------------------------------------------------------------------
  // §7.2 plant derivation surfaced read-only when the source forces a plant.
  // -------------------------------------------------------------------------

  const sourceByCode = $derived(() => {
    const map = new Map<string, SourceLocationRow>();
    for (const s of lookups.source_locations) map.set(s.code, s);
    return map;
  });

  const selectedSource = $derived(
    sourceByCode().get(sourceCode) ?? null
  );

  /** Plant code derived from source (read-only) when applicable, else
   * whatever the operator picked in the override field. */
  const effectivePlantCode = $derived.by(() => {
    if (!selectedSource) return '';
    if (selectedSource.kind === 'warehouse_flec') return plantCodeOverride;
    if (selectedSource.plant_id != null) {
      const p = lookups.plants.find((x) => x.id === selectedSource.plant_id);
      return p?.code ?? '';
    }
    return '';
  });

  const sourceForcesPlant = $derived(
    selectedSource !== null && selectedSource.kind !== 'warehouse_flec'
  );

  // Disposition raw form for the Rust payload + unique_tag.
  const dispositionRaw = $derived.by(() => {
    if (dispositionKind === 'flec_bagging') return 'FLEC';
    return partnerEquipmentCode;
  });

  // Filter partner equipment list to the right kind.
  const partnerEquipmentChoices = $derived.by(() => {
    if (dispositionKind === 'partner_crusher') {
      return lookups.partner_equipment.filter((p) => p.kind === 'crusher');
    }
    if (dispositionKind === 'partner_kiln') {
      return lookups.partner_equipment.filter((p) => p.kind === 'kiln');
    }
    return [];
  });

  // -------------------------------------------------------------------------
  // Submit.
  // -------------------------------------------------------------------------

  async function submit() {
    err = null;
    if (!gradeCode || !sourceCode || !batch.trim() || !weightKg) {
      err = 'fill in batch, grade, source, and weight';
      return;
    }
    if (dispositionKind !== 'flec_bagging' && !partnerEquipmentCode) {
      err = 'pick which crusher/kiln the partner used';
      return;
    }
    const wt = parseFloat(weightKg);
    if (!(wt > 0)) {
      err = 'weight must be > 0';
      return;
    }
    const flecN = flecCount.trim() === '' ? null : parseInt(flecCount, 10);
    if (flecN !== null && (!Number.isFinite(flecN) || flecN <= 0)) {
      err = 'flec count must be a positive integer (or blank)';
      return;
    }

    const input: CreateProductionEventInput = {
      recvDate: recvDate,
      prodDate: prodDate.trim() === '' ? null : prodDate,
      batch: batch,
      shiftCode: shiftCode.trim() === '' ? null : shiftCode,
      gradeCode: gradeCode,
      sourceCode: sourceCode,
      plantCodeOverride:
        selectedSource?.kind === 'warehouse_flec' && plantCodeOverride
          ? plantCodeOverride
          : null,
      warehouseCode: warehouseCode.trim() === '' ? null : warehouseCode,
      dispositionRaw: dispositionRaw,
      weightKg: wt,
      flecCount: flecN,
      whseSide: whseSide.trim() === '' ? null : whseSide,
      flecStat: null,
      dvoBatchId: null,
      notes: notes.trim() === '' ? null : notes
    };

    submitting = true;
    try {
      const row = await invoke<ProductionEventRow>('create_production_event', {
        input
      });
      onSaved(row);
      // Excel-like sticky-by-default: keep everything that's likely to be
      // the same on the next row. Only weight, flec count, and notes are
      // intrinsically per-event and clear on submit. Phase B will refine
      // (e.g. tab cursor jumps to weight; copy-from-above buttons; etc).
      weightKg = '';
      flecCount = '';
      notes = '';
    } catch (e) {
      err = String(e);
    } finally {
      submitting = false;
    }
  }
</script>

<form
  onsubmit={(e) => {
    e.preventDefault();
    submit();
  }}
  class="rounded-md border border-neutral-800 bg-neutral-950 p-3 space-y-3"
>
  <div class="flex items-baseline justify-between">
    <h3 class="text-xs uppercase tracking-wide text-neutral-500">New event</h3>
    <span class="text-xs text-neutral-600">phase A · functional form</span>
  </div>

  <div class="grid grid-cols-12 gap-2 text-xs">
    <!-- Row 1: dates + batch + shift -->
    <label class="col-span-2 space-y-1">
      <span class="text-[10px] uppercase tracking-wide text-neutral-500">Recv date</span>
      <input
        type="date"
        bind:value={recvDate}
        class="w-full rounded bg-neutral-900 border border-neutral-800 px-2 py-1 font-mono"
      />
    </label>
    <label class="col-span-2 space-y-1">
      <span class="text-[10px] uppercase tracking-wide text-neutral-500">Prod date</span>
      <input
        type="date"
        bind:value={prodDate}
        class="w-full rounded bg-neutral-900 border border-neutral-800 px-2 py-1 font-mono"
      />
    </label>
    <label class="col-span-3 space-y-1">
      <span class="text-[10px] uppercase tracking-wide text-neutral-500">Batch</span>
      <input
        type="text"
        bind:value={batch}
        placeholder="MAY"
        class="w-full rounded bg-neutral-900 border border-neutral-800 px-2 py-1 font-mono uppercase"
      />
    </label>
    <label class="col-span-1 space-y-1">
      <span class="text-[10px] uppercase tracking-wide text-neutral-500">Shift</span>
      <select
        bind:value={shiftCode}
        class="w-full rounded bg-neutral-900 border border-neutral-800 px-2 py-1 font-mono"
      >
        <option value="">—</option>
        {#each lookups.shifts as s}
          <option value={s.code}>{s.code}</option>
        {/each}
      </select>
    </label>
    <label class="col-span-2 space-y-1">
      <span class="text-[10px] uppercase tracking-wide text-neutral-500">Grade</span>
      <select
        bind:value={gradeCode}
        class="w-full rounded bg-neutral-900 border border-neutral-800 px-2 py-1 font-mono"
        required
      >
        <option value="" disabled>pick…</option>
        {#each lookups.grades as g}
          <option value={g.code}>{g.code}</option>
        {/each}
      </select>
    </label>
    <label class="col-span-2 space-y-1">
      <span class="text-[10px] uppercase tracking-wide text-neutral-500">Source</span>
      <select
        bind:value={sourceCode}
        class="w-full rounded bg-neutral-900 border border-neutral-800 px-2 py-1 font-mono"
        required
      >
        <option value="" disabled>pick…</option>
        {#each lookups.source_locations as s}
          <option value={s.code}>{s.code}</option>
        {/each}
      </select>
    </label>

    <!-- Row 2: plant + warehouse + side + disposition -->
    <label class="col-span-2 space-y-1">
      <span class="text-[10px] uppercase tracking-wide text-neutral-500">
        Plant
        {#if sourceForcesPlant}
          <span class="text-neutral-600">(forced by source)</span>
        {/if}
      </span>
      {#if sourceForcesPlant}
        <input
          type="text"
          value={effectivePlantCode}
          readonly
          class="w-full rounded bg-neutral-900/40 border border-neutral-800 px-2 py-1 font-mono text-neutral-400"
        />
      {:else}
        <select
          bind:value={plantCodeOverride}
          class="w-full rounded bg-neutral-900 border border-neutral-800 px-2 py-1 font-mono"
        >
          <option value="">—</option>
          {#each lookups.plants as p}
            <option value={p.code}>{p.code}</option>
          {/each}
        </select>
      {/if}
    </label>
    <label class="col-span-2 space-y-1">
      <span class="text-[10px] uppercase tracking-wide text-neutral-500">Warehouse</span>
      <select
        bind:value={warehouseCode}
        class="w-full rounded bg-neutral-900 border border-neutral-800 px-2 py-1 font-mono"
      >
        <option value="">—</option>
        {#each lookups.warehouses as w}
          <option value={w.code}>{w.code}</option>
        {/each}
      </select>
    </label>
    <label class="col-span-1 space-y-1">
      <span class="text-[10px] uppercase tracking-wide text-neutral-500">Side</span>
      <select
        bind:value={whseSide}
        class="w-full rounded bg-neutral-900 border border-neutral-800 px-2 py-1 font-mono"
      >
        <option value="">—</option>
        <option value="LS">LS</option>
        <option value="RS">RS</option>
      </select>
    </label>
    <fieldset class="col-span-3 space-y-1">
      <span class="text-[10px] uppercase tracking-wide text-neutral-500 block">Disposition</span>
      <div class="flex gap-1 text-xs font-mono">
        <label class="flex items-center gap-1 cursor-pointer">
          <input
            type="radio"
            name="disp"
            value="flec_bagging"
            bind:group={dispositionKind}
          />
          FLEC
        </label>
        <label class="flex items-center gap-1 cursor-pointer">
          <input
            type="radio"
            name="disp"
            value="partner_crusher"
            bind:group={dispositionKind}
          />
          Crusher
        </label>
        <label class="flex items-center gap-1 cursor-pointer">
          <input
            type="radio"
            name="disp"
            value="partner_kiln"
            bind:group={dispositionKind}
          />
          Kiln
        </label>
      </div>
    </fieldset>
    <label class="col-span-2 space-y-1">
      <span class="text-[10px] uppercase tracking-wide text-neutral-500">Equipment</span>
      <select
        bind:value={partnerEquipmentCode}
        disabled={dispositionKind === 'flec_bagging'}
        class="w-full rounded bg-neutral-900 border border-neutral-800 px-2 py-1 font-mono disabled:opacity-40"
      >
        <option value="">—</option>
        {#each partnerEquipmentChoices as p}
          <option value={p.code}>{p.code}</option>
        {/each}
      </select>
    </label>
    <label class="col-span-1 space-y-1">
      <span class="text-[10px] uppercase tracking-wide text-neutral-500">Wt (kg)</span>
      <input
        type="number"
        bind:value={weightKg}
        step="0.01"
        min="0"
        placeholder="0"
        class="w-full rounded bg-neutral-900 border border-neutral-800 px-2 py-1 font-mono"
        required
      />
    </label>
    <label class="col-span-1 space-y-1">
      <span class="text-[10px] uppercase tracking-wide text-neutral-500">Flec</span>
      <input
        type="number"
        bind:value={flecCount}
        step="1"
        min="0"
        placeholder=""
        class="w-full rounded bg-neutral-900 border border-neutral-800 px-2 py-1 font-mono"
      />
    </label>

    <!-- Row 3: notes -->
    <label class="col-span-12 space-y-1">
      <span class="text-[10px] uppercase tracking-wide text-neutral-500">Notes</span>
      <input
        type="text"
        bind:value={notes}
        placeholder="optional"
        class="w-full rounded bg-neutral-900 border border-neutral-800 px-2 py-1"
      />
    </label>
  </div>

  {#if err}
    <pre class="text-xs text-red-300 whitespace-pre-wrap font-mono">{err}</pre>
  {/if}

  <div class="flex items-center gap-2 pt-1">
    <button
      type="submit"
      disabled={submitting}
      class="rounded bg-emerald-700 hover:bg-emerald-600 px-4 py-1.5 text-xs font-medium disabled:opacity-50"
    >
      {submitting ? 'saving…' : 'Submit'}
    </button>
    <span class="text-xs text-neutral-600">Sticky fields keep their values; weight/flec/notes clear after submit. Phase B will add Cmd+Enter, tab focus, copy-from-above, type-ahead.</span>
  </div>
</form>
