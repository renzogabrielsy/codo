<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  type Props = {
    warehouseCode: string;
    onClose: () => void;
    onSaved: () => void;
  };
  let { warehouseCode, onClose, onSaved }: Props = $props();

  let gradeCode = $state('3X50');
  let side = $state<'LS' | 'RS'>('RS');
  let count = $state('');
  let dateInput = $state(new Date().toISOString().slice(0, 10));
  let busy = $state(false);
  let err = $state<string | null>(null);

  async function submit() {
    err = null;
    const n = parseInt(count, 10);
    if (!Number.isFinite(n) || n < 0) {
      err = 'flec count must be a non-negative integer';
      return;
    }
    busy = true;
    try {
      await invoke<number>('set_warehouse_opening_balance', {
        input: {
          warehouseCode,
          gradeCode,
          side,
          periodStartDate: dateInput,
          openingFlecCount: n,
          notes: null
        }
      });
      onSaved();
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

<!-- Modal backdrop -->
<button
  class="fixed inset-0 bg-black/40 z-40"
  onclick={onClose}
  aria-label="close"
></button>
<div
  class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none"
>
  <section
    class="pointer-events-auto rounded-md border p-4 space-y-3 w-[420px] border-neutral-300 bg-white dark:border-neutral-800 dark:bg-neutral-950"
  >
    <header class="flex items-baseline justify-between">
      <h3 class="text-sm font-semibold">Set opening balance — {warehouseCode}</h3>
      <button
        type="button"
        onclick={onClose}
        class="text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200 text-base"
      >×</button>
    </header>
    <p class="text-xs text-neutral-600 dark:text-neutral-500">
      Sets the starting flec count for this (grade, side) as of the date below.
      The ledger uses the most-recent opening dated ≤ start date.
    </p>

    <div class="grid grid-cols-2 gap-2 text-sm">
      <label class="space-y-1">
        <span class="text-[11px] uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500">
          Grade
        </span>
        <select
          bind:value={gradeCode}
          disabled={busy}
          class="w-full rounded bg-white dark:bg-neutral-900 border border-neutral-300 dark:border-neutral-700 px-2 py-1 font-mono"
        >
          <option value="3X50">3X50</option>
          <option value="2X6">2X6</option>
          <option value="3.5">3.5</option>
          <option value="4X8">4X8</option>
        </select>
      </label>
      <label class="space-y-1">
        <span class="text-[11px] uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500">
          Side
        </span>
        <select
          bind:value={side}
          disabled={busy}
          class="w-full rounded bg-white dark:bg-neutral-900 border border-neutral-300 dark:border-neutral-700 px-2 py-1 font-mono"
        >
          <option value="RS">RS</option>
          <option value="LS">LS</option>
        </select>
      </label>
      <label class="space-y-1 col-span-2">
        <span class="text-[11px] uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500">
          As of date
        </span>
        <input
          type="date"
          bind:value={dateInput}
          disabled={busy}
          class="w-full rounded bg-white dark:bg-neutral-900 border border-neutral-300 dark:border-neutral-700 px-2 py-1 font-mono"
        />
      </label>
      <label class="space-y-1 col-span-2">
        <span class="text-[11px] uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500">
          Opening flec count
        </span>
        <input
          type="number"
          min="0"
          step="1"
          bind:value={count}
          disabled={busy}
          placeholder="e.g. 53"
          class="w-full rounded bg-white dark:bg-neutral-900 border border-neutral-300 dark:border-neutral-700 px-2 py-1 font-mono"
        />
      </label>
    </div>

    {#if err}
      <pre class="text-xs text-red-700 dark:text-red-300 whitespace-pre-wrap font-mono">{err}</pre>
    {/if}

    <div class="flex justify-end gap-2">
      <button
        type="button"
        onclick={onClose}
        class="rounded border border-neutral-300 dark:border-neutral-700 px-3 py-1 text-sm hover:bg-neutral-100 dark:hover:bg-neutral-800"
      >
        Cancel
      </button>
      <button
        type="button"
        onclick={submit}
        disabled={busy || !count}
        class="rounded bg-emerald-600 hover:bg-emerald-500 dark:bg-emerald-700 dark:hover:bg-emerald-600 text-white px-3 py-1 text-sm font-medium disabled:opacity-50"
      >
        {busy ? 'saving…' : 'Save opening balance'}
      </button>
    </div>
  </section>
</div>
