<script lang="ts">
  import type { DvoBatchSummary } from '$lib/types/codo';
  import OpenBatchDialog from './OpenBatchDialog.svelte';

  type Props = {
    batches: DvoBatchSummary[];
    onChanged: () => void;
  };
  let { batches, onChanged }: Props = $props();

  let openDialog = $state(false);

  const openBatches = $derived(batches.filter((b) => b.status === 'open'));
  const closedBatches = $derived(batches.filter((b) => b.status === 'closed'));

  function fmtPct(v: number | null): string {
    if (v == null) return '—';
    return (v * 100).toFixed(1) + '%';
  }
</script>

<div class="space-y-3">
  <div class="flex items-baseline gap-3">
    <h2 class="text-lg font-bold">WHSE 3 — DVO batches</h2>
    <span class="text-xs font-mono text-neutral-500">kg, per batch</span>
    <a
      href="/warehouses"
      class="ml-auto text-xs text-neutral-500 hover:text-emerald-600 dark:hover:text-emerald-400"
      data-sveltekit-noscroll
    >
      ← back to warehouses
    </a>
    <button
      type="button"
      onclick={() => (openDialog = true)}
      class="rounded bg-emerald-600 hover:bg-emerald-500 dark:bg-emerald-700 dark:hover:bg-emerald-600 text-white px-3 py-1 text-sm font-medium"
    >
      + Open new batch
    </button>
  </div>

  <section class="rounded-md border border-neutral-300 bg-white dark:border-neutral-800 dark:bg-neutral-950 overflow-hidden">
    <header class="px-3 py-1.5 text-xs uppercase tracking-wide font-semibold text-emerald-700 dark:text-emerald-400 border-b border-neutral-300 dark:border-neutral-800">
      Open batches ({openBatches.length})
    </header>
    {#if openBatches.length === 0}
      <p class="px-3 py-3 text-sm italic text-neutral-500">no open batches</p>
    {:else}
      <table class="w-full text-sm font-mono">
        <thead class="bg-neutral-50 dark:bg-neutral-900/50 text-[12px] uppercase tracking-wider text-neutral-700 dark:text-neutral-400">
          <tr>
            <th class="px-3 py-2 text-left font-semibold">Code</th>
            <th class="px-3 py-2 text-left font-semibold">Side</th>
            <th class="px-3 py-2 text-right font-semibold">Receipts</th>
            <th class="px-3 py-2 text-right font-semibold">Outflows</th>
            <th class="px-3 py-2 text-left font-semibold">Opened</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each openBatches as b (b.id)}
            <tr class="border-t border-neutral-200 dark:border-neutral-900 hover:bg-emerald-50/40 dark:hover:bg-emerald-950/20">
              <td class="px-3 py-2 font-bold text-neutral-900 dark:text-neutral-100">{b.code}</td>
              <td class="px-3 py-2">{b.side}</td>
              <td class="px-3 py-2 text-right">{b.receipt_count}</td>
              <td class="px-3 py-2 text-right">{b.outflow_count}</td>
              <td class="px-3 py-2 text-neutral-500">{b.opened_at.slice(0, 10)}</td>
              <td class="px-3 py-2 text-right">
                <a
                  href="/warehouses?batch={b.id}"
                  class="text-emerald-700 dark:text-emerald-400 hover:underline"
                  data-sveltekit-noscroll
                >
                  view ledger →
                </a>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </section>

  <section class="rounded-md border border-neutral-300 bg-white dark:border-neutral-800 dark:bg-neutral-950 overflow-hidden">
    <header class="px-3 py-1.5 text-xs uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500 border-b border-neutral-300 dark:border-neutral-800">
      Closed batches ({closedBatches.length})
    </header>
    {#if closedBatches.length === 0}
      <p class="px-3 py-3 text-sm italic text-neutral-500">no closed batches yet</p>
    {:else}
      <table class="w-full text-sm font-mono">
        <thead class="bg-neutral-50 dark:bg-neutral-900/50 text-[12px] uppercase tracking-wider text-neutral-700 dark:text-neutral-400">
          <tr>
            <th class="px-3 py-2 text-left font-semibold">Code</th>
            <th class="px-3 py-2 text-left font-semibold">Side</th>
            <th class="px-3 py-2 text-right font-semibold">Receipts</th>
            <th class="px-3 py-2 text-right font-semibold">Outflows</th>
            <th class="px-3 py-2 text-right font-semibold">Transit loss</th>
            <th class="px-3 py-2 text-right font-semibold">Yield loss</th>
            <th class="px-3 py-2 text-left font-semibold">Closed</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each closedBatches as b (b.id)}
            <tr class="border-t border-neutral-200 dark:border-neutral-900 hover:bg-neutral-50 dark:hover:bg-neutral-900/40">
              <td class="px-3 py-2 text-neutral-700 dark:text-neutral-300">{b.code}</td>
              <td class="px-3 py-2 text-neutral-500">{b.side}</td>
              <td class="px-3 py-2 text-right text-neutral-500">{b.receipt_count}</td>
              <td class="px-3 py-2 text-right text-neutral-500">{b.outflow_count}</td>
              <td class="px-3 py-2 text-right text-neutral-700 dark:text-neutral-300">{fmtPct(b.frozen_transit_loss)}</td>
              <td class="px-3 py-2 text-right text-neutral-700 dark:text-neutral-300">{fmtPct(b.frozen_yield_loss)}</td>
              <td class="px-3 py-2 text-neutral-500">{b.closed_at?.slice(0, 10) ?? '—'}</td>
              <td class="px-3 py-2 text-right">
                <a
                  href="/warehouses?batch={b.id}"
                  class="text-neutral-500 hover:text-emerald-700 dark:hover:text-emerald-400 hover:underline"
                  data-sveltekit-noscroll
                >
                  view →
                </a>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </section>
</div>

{#if openDialog}
  <OpenBatchDialog
    onClose={() => (openDialog = false)}
    onCreated={() => {
      openDialog = false;
      onChanged();
    }}
  />
{/if}
