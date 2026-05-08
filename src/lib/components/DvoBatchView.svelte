<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import type { DvoBatchLedger, DvoLedgerEvent, DvoOutflowRow, DvoReceiptRow } from '$lib/types/codo';

  type Props = { batchId: number };
  let { batchId }: Props = $props();

  let ledger = $state<DvoBatchLedger | null>(null);
  let busy = $state(false);
  let err = $state<string | null>(null);
  let closing = $state(false);

  async function load() {
    err = null;
    busy = true;
    try {
      ledger = await invoke<DvoBatchLedger>('get_dvo_batch_ledger', { dvoBatchId: batchId });
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
      ledger = null;
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    void batchId;
    load();
  });

  async function closeBatch() {
    if (!ledger) return;
    if (!confirm(`Close batch ${ledger.batch.code}? This freezes transit and yield loss at current values. Cannot be undone.`)) return;
    closing = true;
    try {
      await invoke<void>('close_dvo_batch', { dvoBatchId: batchId, closedBy: null });
      await load();
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    } finally {
      closing = false;
    }
  }

  function fmtPct(v: number): string {
    return (v * 100).toFixed(2) + '%';
  }
  function fmtKg(v: number): string {
    if (v >= 1000) return (v / 1000).toFixed(2) + ' t';
    return v.toFixed(0) + ' kg';
  }
  function fmtKgPlain(v: number): string {
    return v.toFixed(0);
  }
  function fmtDate(iso: string): string {
    const m = iso.match(/^(\d{4})-(\d{2})-(\d{2})$/);
    if (!m) return iso;
    return `${parseInt(m[2], 10)}/${parseInt(m[3], 10)}`;
  }
  function dispLabel(d: { kind: string; equipment_code?: number }): string {
    if (d.kind === 'FlecBagging') return 'FLEC';
    if (d.kind === 'PartnerCrusher') return `C${d.equipment_code}`;
    if (d.kind === 'PartnerKiln') return `RK${d.equipment_code}`;
    return '?';
  }
  function isReceipt(e: DvoLedgerEvent): e is { kind: 'Receipt'; receipt: DvoReceiptRow; run_bal_kg: number } {
    return e.kind === 'Receipt';
  }
  function isOutflow(e: DvoLedgerEvent): e is { kind: 'Outflow'; outflow: DvoOutflowRow; run_bal_kg: number } {
    return e.kind === 'Outflow';
  }
</script>

<div class="space-y-2">
  <!-- Sticky header strip: title + status + KPIs all in one block -->
  <div class="sticky top-0 z-20 bg-neutral-50 dark:bg-neutral-950 border-b border-neutral-300 dark:border-neutral-800 -mx-4 px-4 py-2 space-y-2">
    {#if ledger}
      <!-- Row 1: title + actions -->
      <div class="flex items-center gap-3 text-sm">
        <h2 class="text-base font-bold">{ledger.batch.code}</h2>
        <span class="text-[11px] font-mono text-neutral-500">WHSE 3 · DVO batch</span>
        <span
          class="text-[10px] uppercase tracking-wider font-semibold px-1.5 py-0.5 rounded {ledger.batch.status === 'open'
            ? 'bg-emerald-100 text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300'
            : 'bg-neutral-200 text-neutral-700 dark:bg-neutral-800 dark:text-neutral-400'}"
        >
          {ledger.batch.status}
        </span>
        <a
          href="/warehouses?dvo=1"
          class="text-xs text-neutral-500 hover:text-emerald-600 dark:hover:text-emerald-400"
          data-sveltekit-noscroll
        >
          ← back
        </a>
        <div class="flex-1"></div>
        <span class="text-[11px] font-mono text-neutral-500">
          {ledger.receipts.length} receipts · {ledger.outflows.length} outflows
        </span>
        {#if ledger.batch.status === 'open'}
          <button
            type="button"
            onclick={closeBatch}
            disabled={closing}
            class="rounded bg-amber-600 hover:bg-amber-500 dark:bg-amber-700 dark:hover:bg-amber-600 text-white px-2 py-0.5 text-xs font-medium disabled:opacity-50"
          >
            {closing ? 'closing…' : 'Close batch'}
          </button>
        {/if}
      </div>

      {#if err}
        <div class="px-2 py-1 rounded-md text-xs font-mono text-red-700 bg-red-100 border border-red-300 dark:text-red-200 dark:bg-red-900/40 dark:border-red-700/50">
          ⚠ {err}
        </div>
      {/if}

      <!-- Row 2: compact KPI strip -->
      <div class="grid grid-cols-2 lg:grid-cols-4 gap-2 text-xs font-mono">
        <div class="rounded border border-neutral-200 dark:border-neutral-800 px-2 py-1">
          <p class="text-[10px] uppercase tracking-wide text-neutral-500">Receipts (Cebu)</p>
          <div class="flex items-baseline justify-between">
            <span class="font-bold text-base">{fmtKg(ledger.receipts.reduce((s, r) => s + r.cebu_declared_weight_kg, 0))}</span>
          </div>
        </div>
        <div class="rounded border border-neutral-200 dark:border-neutral-800 px-2 py-1">
          <p class="text-[10px] uppercase tracking-wide text-neutral-500">Outflows (partner)</p>
          <div class="flex items-baseline justify-between">
            <span class="font-bold text-base">{fmtKg(ledger.outflows.reduce((s, o) => s + o.weight_kg, 0))}</span>
          </div>
        </div>
        <div class="rounded border border-neutral-200 dark:border-neutral-800 px-2 py-1">
          <p class="text-[10px] uppercase tracking-wide text-neutral-500">Transit loss</p>
          <div class="flex items-baseline justify-between">
            <span class="font-bold text-base">{fmtPct(ledger.transit_loss.value)}</span>
            <span class="text-[10px] text-neutral-500">= {fmtKgPlain(ledger.transit_loss.numerator_kg)}/{fmtKgPlain(ledger.transit_loss.denominator_kg)}</span>
          </div>
        </div>
        <div class="rounded border border-neutral-200 dark:border-neutral-800 px-2 py-1">
          <p class="text-[10px] uppercase tracking-wide text-neutral-500">Yield loss</p>
          <div class="flex items-baseline justify-between">
            <span class="font-bold text-base">{fmtPct(ledger.yield_loss.value)}</span>
            <span class="text-[10px] text-neutral-500">= {fmtKgPlain(ledger.yield_loss.numerator_kg)}/{fmtKgPlain(ledger.yield_loss.denominator_kg)}</span>
          </div>
        </div>
      </div>
    {/if}
  </div>

  {#if busy && !ledger}
    <p class="text-sm text-neutral-500 font-mono">loading batch ledger…</p>
  {:else if ledger}

    <!-- Interleaved ledger -->
    <section class="rounded-md border border-neutral-300 bg-white dark:border-neutral-800 dark:bg-neutral-950 overflow-hidden">
      <div class="px-3 py-1.5 text-xs uppercase tracking-wide font-semibold text-neutral-700 dark:text-neutral-400 border-b border-neutral-300 dark:border-neutral-800">
        Interleaved ledger ({ledger.interleaved.length} events)
      </div>
      {#if ledger.interleaved.length === 0}
        <p class="px-3 py-4 text-center text-sm italic text-neutral-500">
          no receipts or outflows recorded yet
        </p>
      {:else}
        <div class="overflow-x-auto">
          <table class="w-full text-sm font-mono">
            <thead class="bg-neutral-100 text-neutral-700 dark:bg-neutral-900 dark:text-neutral-300 text-[12px] uppercase tracking-wider">
              <tr>
                <th class="px-2 py-2 text-left font-semibold">Date</th>
                <th class="px-2 py-2 text-left font-semibold">Type</th>
                <th class="px-2 py-2 text-left font-semibold">Detail</th>
                <th class="px-2 py-2 text-right font-semibold">In (kg)</th>
                <th class="px-2 py-2 text-right font-semibold">Out (kg)</th>
                <th class="px-2 py-2 text-right font-semibold">Run bal (kg)</th>
              </tr>
            </thead>
            <tbody>
              {#each ledger.interleaved as ev, i (i)}
                {#if isReceipt(ev)}
                  <tr class="border-t border-neutral-200 dark:border-neutral-900 hover:bg-emerald-50/40 dark:hover:bg-emerald-950/20">
                    <td class="px-2 py-1">{fmtDate(ev.receipt.recv_date)}</td>
                    <td class="px-2 py-1">
                      <span class="inline-block rounded px-1.5 py-0.5 text-[11px] font-bold tracking-wider bg-emerald-100 text-emerald-800 border border-emerald-300 dark:bg-emerald-700/40 dark:text-emerald-200 dark:border-emerald-600/40">
                        RECEIPT
                      </span>
                    </td>
                    <td class="px-2 py-1 text-neutral-700 dark:text-neutral-300">
                      {ev.receipt.gothong_slip ?? '—'}
                      {#if ev.receipt.cebu_declared_weight_kg !== ev.receipt.dvo_declared_weight_kg}
                        <span class="text-neutral-500 text-[11px]">
                          (DVO {fmtKgPlain(ev.receipt.dvo_declared_weight_kg)} → Cebu {fmtKgPlain(ev.receipt.cebu_declared_weight_kg)})
                        </span>
                      {/if}
                    </td>
                    <td class="px-2 py-1 text-right text-emerald-700 dark:text-emerald-400 font-semibold">
                      {fmtKgPlain(ev.receipt.cebu_declared_weight_kg)}
                    </td>
                    <td class="px-2 py-1"></td>
                    <td class="px-2 py-1 text-right font-bold text-neutral-900 dark:text-neutral-100">
                      {fmtKgPlain(ev.run_bal_kg)}
                    </td>
                  </tr>
                {:else if isOutflow(ev)}
                  <tr class="border-t border-neutral-200 dark:border-neutral-900 hover:bg-purple-50/40 dark:hover:bg-purple-950/20">
                    <td class="px-2 py-1">{fmtDate(ev.outflow.recv_date)}</td>
                    <td class="px-2 py-1">
                      <span class="inline-block rounded px-1.5 py-0.5 text-[11px] font-bold tracking-wider bg-purple-100 text-purple-800 border border-purple-300 dark:bg-purple-700/40 dark:text-purple-200 dark:border-purple-600/40">
                        OUT {dispLabel(ev.outflow.disposition)}
                      </span>
                    </td>
                    <td class="px-2 py-1 text-neutral-500 truncate" title={ev.outflow.unique_tag}>
                      <a
                        href="/?event={ev.outflow.event_id}"
                        class="text-emerald-700 dark:text-emerald-400 hover:underline"
                        data-sveltekit-noscroll
                        title="open in production log"
                      >
                        {ev.outflow.unique_tag} ↗
                      </a>
                    </td>
                    <td class="px-2 py-1"></td>
                    <td class="px-2 py-1 text-right text-purple-700 dark:text-purple-400 font-semibold">
                      {fmtKgPlain(ev.outflow.weight_kg)}
                    </td>
                    <td class="px-2 py-1 text-right font-bold text-neutral-900 dark:text-neutral-100">
                      {fmtKgPlain(ev.run_bal_kg)}
                    </td>
                  </tr>
                {/if}
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </section>
  {/if}
</div>
