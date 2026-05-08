<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import type { FlecLedger } from '$lib/types/codo';
  import OpeningBalanceForm from './OpeningBalanceForm.svelte';

  type Props = { warehouseCode: string };
  let { warehouseCode }: Props = $props();

  let startDateInput = $state(epochStart());
  let ledger = $state<FlecLedger | null>(null);
  let err = $state<string | null>(null);
  let busy = $state(false);
  let openingFormOpen = $state(false);

  function epochStart(): string {
    return '2025-01-01';
  }

  async function load() {
    err = null;
    busy = true;
    try {
      ledger = await invoke<FlecLedger>('get_warehouse_ledger_flec', {
        warehouseCode,
        startDate: startDateInput
      });
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
      ledger = null;
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    // Re-load when warehouseCode prop changes (parent navigates between WHSEs).
    void warehouseCode;
    load();
  });

  // Group balances by grade so the compact strip can render one row per grade
  // with RS + LS side by side (matches the workbook's PC WHSE layout).
  type BalanceRow = {
    grade: string;
    rs: import('$lib/types/codo').CurrentBalance | null;
    ls: import('$lib/types/codo').CurrentBalance | null;
  };
  const balanceRows = $derived.by((): BalanceRow[] => {
    if (!ledger) return [];
    const byGrade = new Map<string, BalanceRow>();
    for (const b of ledger.current_balances) {
      const r = byGrade.get(b.grade) ?? { grade: b.grade, rs: null, ls: null };
      if (b.side === 'Rs') r.rs = b;
      else if (b.side === 'Ls') r.ls = b;
      byGrade.set(b.grade, r);
    }
    return [...byGrade.values()].sort((a, b) =>
      gradeOrder(a.grade) - gradeOrder(b.grade)
    );
  });
  function gradeOrder(g: string): number {
    return { G3x50: 0, G2x6: 1, G3p5: 2, G4x8: 3 }[g as 'G3x50' | 'G2x6' | 'G3p5' | 'G4x8'] ?? 99;
  }

  // Render-time helpers.
  function gradeLabel(g: string): string {
    switch (g) {
      case 'G3x50':
        return '3X50';
      case 'G2x6':
        return '2X6';
      case 'G3p5':
        return '3.5';
      case 'G4x8':
        return '4X8';
      default:
        return g;
    }
  }
  function sideLabel(s: string | null): string {
    if (s === 'Ls') return 'LS';
    if (s === 'Rs') return 'RS';
    return s ?? '';
  }
  function fmtDate(iso: string): string {
    const m = iso.match(/^(\d{4})-(\d{2})-(\d{2})$/);
    if (!m) return iso;
    return `${parseInt(m[2], 10)}/${parseInt(m[3], 10)}`;
  }
  function srcLabel(s: string): string {
    return s.replace(/^Tnk(\d)$/, 'TNK $1').replace('Flec', 'FLEC').replace('Dvo', 'DVO').toUpperCase();
  }
  function dispLabel(d: { kind: string; equipment_code?: number }): string {
    if (d.kind === 'FlecBagging') return 'FLEC';
    if (d.kind === 'PartnerCrusher') return `C${d.equipment_code}`;
    if (d.kind === 'PartnerKiln') return `RK${d.equipment_code}`;
    return '?';
  }
  function dispBadgeCls(d: { kind: string }): string {
    const base = 'inline-block rounded px-1.5 py-0.5 text-[11px] font-bold tracking-wider border';
    switch (d.kind) {
      case 'FlecBagging':
        return `${base} bg-emerald-100 text-emerald-800 border-emerald-300 dark:bg-emerald-700/40 dark:text-emerald-200 dark:border-emerald-600/40`;
      case 'PartnerCrusher':
        return `${base} bg-sky-100 text-sky-800 border-sky-300 dark:bg-sky-700/40 dark:text-sky-200 dark:border-sky-600/40`;
      case 'PartnerKiln':
        return `${base} bg-amber-100 text-amber-800 border-amber-300 dark:bg-amber-700/40 dark:text-amber-200 dark:border-amber-600/40`;
      default:
        return `${base} bg-neutral-100`;
    }
  }
</script>

<div class="space-y-2">
  <!-- Sticky header strip: title + date + balance summary all in one block. -->
  <div class="sticky top-0 z-20 bg-neutral-50 dark:bg-neutral-950 border-b border-neutral-300 dark:border-neutral-800 -mx-4 px-4 py-2 space-y-2">
    <!-- Row 1: title + actions -->
    <div class="flex items-center gap-3 text-sm">
      <h2 class="text-base font-bold">{warehouseCode}</h2>
      <span class="text-[11px] font-mono text-neutral-500">flec-count ledger</span>
      <a
        href="/warehouses"
        class="text-xs text-neutral-500 hover:text-emerald-600 dark:hover:text-emerald-400"
        data-sveltekit-noscroll
      >
        ← back
      </a>
      <span class="text-neutral-300 dark:text-neutral-700">·</span>
      <span class="text-[11px] font-mono text-neutral-500">show from</span>
      <input
        type="date"
        bind:value={startDateInput}
        onchange={load}
        class="rounded bg-white dark:bg-neutral-900 border border-neutral-300 dark:border-neutral-700 px-1.5 py-0.5 text-xs font-mono"
      />
      <button
        type="button"
        onclick={load}
        class="rounded border border-neutral-300 hover:border-neutral-500 dark:border-neutral-700 dark:hover:border-neutral-500 px-2 py-0.5 text-xs hover:bg-neutral-100 dark:hover:bg-neutral-800"
      >
        ↻
      </button>
      <div class="flex-1"></div>
      {#if ledger}
        <span class="text-[11px] font-mono text-neutral-500">{ledger.rows.length} events</span>
      {/if}
      <button
        type="button"
        onclick={() => (openingFormOpen = true)}
        class="rounded bg-emerald-600 hover:bg-emerald-500 dark:bg-emerald-700 dark:hover:bg-emerald-600 text-white px-2 py-0.5 text-xs"
      >
        + opening balance
      </button>
    </div>

    {#if err}
      <div class="px-2 py-1 rounded-md text-xs font-mono text-red-700 bg-red-100 border border-red-300 dark:text-red-200 dark:bg-red-900/40 dark:border-red-700/50">
        ⚠ {err}
      </div>
    {/if}

    <!-- Row 2: compact balance table — all (grade, side) on a single horizontal strip -->
    {#if ledger}
      {#if ledger.current_balances.length === 0}
        <p class="text-xs text-neutral-500 italic">no opening balance + no events</p>
      {:else}
        <table class="w-full text-xs font-mono border border-neutral-200 dark:border-neutral-800 rounded">
          <thead class="bg-neutral-100 dark:bg-neutral-900 text-neutral-600 dark:text-neutral-500 text-[10px] uppercase tracking-wider">
            <tr>
              <th class="px-2 py-1 text-left font-semibold">Grade</th>
              <th class="px-2 py-1 text-right font-semibold">RS</th>
              <th class="px-2 py-1 text-left font-semibold text-neutral-500">= breakdown</th>
              <th class="px-2 py-1 text-right font-semibold">LS</th>
              <th class="px-2 py-1 text-left font-semibold text-neutral-500">= breakdown</th>
            </tr>
          </thead>
          <tbody>
            {#each balanceRows as br (br.grade)}
              <tr class="border-t border-neutral-200 dark:border-neutral-800/60">
                <td class="px-2 py-0.5 text-violet-700 dark:text-violet-300 font-semibold">{gradeLabel(br.grade)}</td>
                <td class="px-2 py-0.5 text-right font-bold text-neutral-900 dark:text-neutral-100">
                  {#if br.rs}{br.rs.flec_count}{:else}—{/if}
                </td>
                <td class="px-2 py-0.5 text-[10px] text-neutral-500">
                  {#if br.rs}
                    = {br.rs.components.opening} + {br.rs.components.flec_in_to_date} − {br.rs.components.flec_out_to_date}
                  {/if}
                </td>
                <td class="px-2 py-0.5 text-right font-bold text-neutral-900 dark:text-neutral-100">
                  {#if br.ls}{br.ls.flec_count}{:else}—{/if}
                </td>
                <td class="px-2 py-0.5 text-[10px] text-neutral-500">
                  {#if br.ls}
                    = {br.ls.components.opening} + {br.ls.components.flec_in_to_date} − {br.ls.components.flec_out_to_date}
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
      {#if ledger.unsided_event_count > 0}
        <p class="text-[11px] text-amber-700 dark:text-amber-400 font-mono">
          ⚠ {ledger.unsided_event_count} legacy event(s) without LS/RS — excluded from balance,
          shown below for review.
        </p>
      {/if}
    {/if}
  </div>

  <!-- Ledger table -->
  {#if busy && !ledger}
    <p class="text-sm text-neutral-500 font-mono">loading ledger…</p>
  {:else if ledger}
    <section class="rounded-md border border-neutral-300 bg-white dark:border-neutral-800 dark:bg-neutral-950 overflow-hidden">
      {#if ledger.rows.length === 0}
        <p class="px-3 py-4 text-center text-sm italic text-neutral-500">
          no events for this warehouse since {ledger.start_date}
        </p>
      {:else}
        <div class="overflow-x-auto">
          <table class="w-full text-xs font-mono">
            <thead class="bg-neutral-100 text-neutral-700 dark:bg-neutral-900 dark:text-neutral-300 text-[11px] uppercase tracking-wider">
              <tr>
                <th class="px-2 py-1.5 text-left font-semibold">Recv</th>
                <th class="px-2 py-1.5 text-left font-semibold">Source</th>
                <th class="px-2 py-1.5 text-left font-semibold">Grd</th>
                <th class="px-2 py-1.5 text-left font-semibold">Sd</th>
                <th class="px-2 py-1.5 text-right font-semibold">kg in</th>
                <th class="px-2 py-1.5 text-right font-semibold">kg out</th>
                <th class="px-2 py-1.5 text-right font-semibold">flec in</th>
                <th class="px-2 py-1.5 text-right font-semibold">flec out</th>
                <th class="px-2 py-1.5 text-left font-semibold">Dest</th>
                <th class="px-2 py-1.5 text-right font-semibold">Bal</th>
                <th class="px-2 py-1.5 text-left font-semibold text-neutral-500">= breakdown</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {#each ledger.rows as r (r.event_id)}
                <tr class="border-t border-neutral-200 dark:border-neutral-900 hover:bg-emerald-50/40 dark:hover:bg-emerald-950/20">
                  <td class="px-2 py-0.5 text-neutral-900 dark:text-neutral-200">{fmtDate(r.recv_date)}</td>
                  <td class="px-2 py-0.5 text-cyan-700 dark:text-cyan-300">{srcLabel(r.source)}</td>
                  <td class="px-2 py-0.5 text-violet-700 dark:text-violet-300">{gradeLabel(r.grade)}</td>
                  <td class="px-2 py-0.5">{sideLabel(r.side)}</td>
                  <td class="px-2 py-0.5 text-right">{r.kg_in?.toFixed(0) ?? ''}</td>
                  <td class="px-2 py-0.5 text-right">{r.kg_out?.toFixed(0) ?? ''}</td>
                  <td class="px-2 py-0.5 text-right text-emerald-700 dark:text-emerald-400">
                    {r.flec_in ?? ''}
                  </td>
                  <td class="px-2 py-0.5 text-right text-purple-700 dark:text-purple-400">
                    {r.flec_out ?? ''}
                  </td>
                  <td class="px-2 py-0.5">
                    <span class={dispBadgeCls(r.disposition)}>{dispLabel(r.disposition)}</span>
                  </td>
                  <td class="px-2 py-0.5 text-right font-bold text-neutral-900 dark:text-neutral-100">
                    {r.run_bal_flec}
                  </td>
                  <td class="px-2 py-0.5 text-[10px] text-neutral-500">
                    = {r.run_bal_components.opening} +
                    {r.run_bal_components.flec_in_to_date} −
                    {r.run_bal_components.flec_out_to_date}
                  </td>
                  <td class="px-2 py-0.5 text-right">
                    <a
                      href="/?event={r.event_id}"
                      class="text-[11px] text-emerald-700 dark:text-emerald-400 hover:underline"
                      data-sveltekit-noscroll
                      title="open in production log"
                    >
                      ↗
                    </a>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </section>
  {/if}
</div>

{#if openingFormOpen}
  <OpeningBalanceForm
    {warehouseCode}
    onClose={() => (openingFormOpen = false)}
    onSaved={() => {
      openingFormOpen = false;
      load();
    }}
  />
{/if}
