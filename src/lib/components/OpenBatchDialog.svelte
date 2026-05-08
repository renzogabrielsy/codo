<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  type Props = {
    onClose: () => void;
    onCreated: () => void;
  };
  let { onClose, onCreated }: Props = $props();

  // Default to current month, RIGHT side. Code auto-derives unless edited.
  const monthNames = [
    'JANUARY', 'FEBRUARY', 'MARCH', 'APRIL', 'MAY', 'JUNE',
    'JULY', 'AUGUST', 'SEPTEMBER', 'OCTOBER', 'NOVEMBER', 'DECEMBER'
  ];
  const now = new Date();
  let startMonth = $state<number>(now.getMonth() + 1);
  let year = $state<number>(now.getFullYear());
  let side = $state<'LEFT' | 'RIGHT'>('RIGHT');
  let codeOverride = $state('');
  let busy = $state(false);
  let err = $state<string | null>(null);

  const derivedCode = $derived(`${monthNames[startMonth - 1]}${year}${side}`);
  const finalCode = $derived(codeOverride.trim() || derivedCode);

  async function submit() {
    err = null;
    busy = true;
    try {
      await invoke<number>('open_dvo_batch', {
        input: {
          code: finalCode,
          startMonth,
          year,
          side,
          notes: null
        }
      });
      onCreated();
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

<button class="fixed inset-0 bg-black/40 z-40" onclick={onClose} aria-label="close"></button>
<div class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none">
  <section class="pointer-events-auto rounded-md border p-4 space-y-3 w-[480px] border-neutral-300 bg-white dark:border-neutral-800 dark:bg-neutral-950">
    <header class="flex items-baseline justify-between">
      <h3 class="text-sm font-semibold">Open new DVO batch</h3>
      <button type="button" onclick={onClose} class="text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-200 text-base">×</button>
    </header>
    <p class="text-xs text-neutral-600 dark:text-neutral-500">
      A DVO batch tracks one side of WHSE 3 starting in a given month. The code is the
      convention <span class="font-mono">MONTH(start)YEAR(SIDE)</span> — e.g. <span class="font-mono">NOVEMBER2025RIGHT</span>.
    </p>

    <div class="grid grid-cols-3 gap-2 text-sm">
      <label class="space-y-1">
        <span class="text-[11px] uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500">Start month</span>
        <select
          bind:value={startMonth}
          disabled={busy}
          class="w-full rounded bg-white dark:bg-neutral-900 border border-neutral-300 dark:border-neutral-700 px-2 py-1 font-mono"
        >
          {#each monthNames as m, i (i)}
            <option value={i + 1}>{m}</option>
          {/each}
        </select>
      </label>
      <label class="space-y-1">
        <span class="text-[11px] uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500">Year</span>
        <input
          type="number"
          bind:value={year}
          disabled={busy}
          min="2024"
          max="2030"
          class="w-full rounded bg-white dark:bg-neutral-900 border border-neutral-300 dark:border-neutral-700 px-2 py-1 font-mono"
        />
      </label>
      <label class="space-y-1">
        <span class="text-[11px] uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500">Side</span>
        <select
          bind:value={side}
          disabled={busy}
          class="w-full rounded bg-white dark:bg-neutral-900 border border-neutral-300 dark:border-neutral-700 px-2 py-1 font-mono"
        >
          <option value="RIGHT">RIGHT</option>
          <option value="LEFT">LEFT</option>
        </select>
      </label>
      <label class="space-y-1 col-span-3">
        <span class="text-[11px] uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500">
          Code (auto: <span class="font-mono">{derivedCode}</span>; override below if you want)
        </span>
        <input
          type="text"
          bind:value={codeOverride}
          disabled={busy}
          placeholder={derivedCode}
          class="w-full rounded bg-white dark:bg-neutral-900 border border-neutral-300 dark:border-neutral-700 px-2 py-1 font-mono uppercase"
        />
      </label>
    </div>

    {#if err}
      <pre class="text-xs text-red-700 dark:text-red-300 whitespace-pre-wrap font-mono">{err}</pre>
    {/if}

    <div class="flex justify-end gap-2">
      <button type="button" onclick={onClose} class="rounded border border-neutral-300 dark:border-neutral-700 px-3 py-1 text-sm hover:bg-neutral-100 dark:hover:bg-neutral-800">
        Cancel
      </button>
      <button
        type="button"
        onclick={submit}
        disabled={busy}
        class="rounded bg-emerald-600 hover:bg-emerald-500 dark:bg-emerald-700 dark:hover:bg-emerald-600 text-white px-3 py-1 text-sm font-medium disabled:opacity-50"
      >
        {busy ? 'opening…' : `Open ${finalCode}`}
      </button>
    </div>
  </section>
</div>
