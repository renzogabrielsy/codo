<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/state';
  import { invoke } from '@tauri-apps/api/core';
  import TopNav from '$lib/components/TopNav.svelte';
  import WarehouseList from '$lib/components/WarehouseList.svelte';
  import FlecLedgerView from '$lib/components/FlecLedgerView.svelte';
  import DvoBatchList from '$lib/components/DvoBatchList.svelte';
  import DvoBatchView from '$lib/components/DvoBatchView.svelte';
  import type { DvoBatchSummary, WarehouseSummary } from '$lib/types/codo';

  type BootState =
    | { kind: 'loading'; step: string }
    | { kind: 'needs_onboarding' }
    | {
        kind: 'ready';
        version: string;
        summaries: WarehouseSummary[];
        batches: DvoBatchSummary[];
      }
    | { kind: 'error'; message: string };

  let state: BootState = $state({ kind: 'loading', step: 'starting…' });

  async function refresh() {
    state = { kind: 'loading', step: 'starting…' };
    try {
      const onboarded = await invoke<boolean>('is_onboarded');
      if (!onboarded) {
        state = { kind: 'needs_onboarding' };
        return;
      }
      state = { kind: 'loading', step: 'opening local database…' };
      await invoke<void>('init_db');

      state = { kind: 'loading', step: 'loading warehouse summaries…' };
      const [version, summaries, batches] = await Promise.all([
        invoke<string>('current_version'),
        invoke<WarehouseSummary[]>('list_warehouse_summaries'),
        invoke<DvoBatchSummary[]>('list_dvo_batches')
      ]);
      state = { kind: 'ready', version, summaries, batches };
    } catch (err) {
      state = { kind: 'error', message: err instanceof Error ? err.message : String(err) };
    }
  }

  async function refreshBatches() {
    if (state.kind !== 'ready') return;
    const batches = await invoke<DvoBatchSummary[]>('list_dvo_batches');
    state = { ...state, batches };
  }

  onMount(refresh);

  // URL param routing: ?w=WHSE 1 → FLEC ledger; ?dvo=1 → DVO list;
  // ?batch=N → DVO batch view; nothing → warehouse list.
  const selectedW = $derived(page.url.searchParams.get('w'));
  const selectedDvo = $derived(page.url.searchParams.get('dvo'));
  const selectedBatchId = $derived.by(() => {
    const v = page.url.searchParams.get('batch');
    if (!v) return null;
    const n = parseInt(v, 10);
    return Number.isFinite(n) ? n : null;
  });
</script>

{#if state.kind === 'ready'}
  <TopNav version={state.version} />
{/if}

{#if state.kind === 'loading'}
  <main class="min-h-screen flex flex-col items-center justify-center gap-3">
    <h1 class="text-3xl font-bold tracking-tight">codo</h1>
    <p class="font-mono text-sm text-neutral-600 dark:text-neutral-400">{state.step}</p>
  </main>
{:else if state.kind === 'needs_onboarding'}
  <main class="min-h-screen flex flex-col items-center justify-center gap-3 px-6">
    <p class="text-sm text-neutral-700 dark:text-neutral-400">
      First-launch setup hasn't run.
      <a href="/" class="text-emerald-700 dark:text-emerald-400 hover:underline">go home</a>
      to onboard.
    </p>
  </main>
{:else if state.kind === 'error'}
  <main class="min-h-screen flex flex-col items-center justify-center gap-3 px-6 py-12">
    <section class="w-full max-w-xl rounded-xl border p-6 space-y-3 border-red-300 bg-red-50 dark:border-red-900 dark:bg-red-950">
      <h2 class="text-lg font-semibold text-red-700 dark:text-red-200">Boot error</h2>
      <pre class="text-xs whitespace-pre-wrap text-red-700 dark:text-red-300">{state.message}</pre>
      <button class="rounded-md px-3 py-1.5 text-sm bg-neutral-200 hover:bg-neutral-300 dark:bg-neutral-800 dark:hover:bg-neutral-700" onclick={refresh}>
        Retry
      </button>
    </section>
  </main>
{:else if state.kind === 'ready'}
  <main class="px-4 py-3 space-y-3">
    {#if selectedBatchId !== null}
      <DvoBatchView batchId={selectedBatchId} />
    {:else if selectedDvo}
      <DvoBatchList batches={state.batches} onChanged={refreshBatches} />
    {:else if selectedW}
      <FlecLedgerView warehouseCode={selectedW} />
    {:else}
      <div class="space-y-3">
        <h1 class="text-2xl font-bold">Warehouses</h1>
        <p class="text-sm text-neutral-600 dark:text-neutral-400">
          Click a warehouse to view its ledger. WHSE 1/2/5/7 track flec count per
          (grade, side); WHSE 3 tracks kg per DVO batch.
        </p>
        <WarehouseList summaries={state.summaries} />
      </div>
    {/if}
  </main>
{/if}
