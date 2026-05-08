<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import Onboarding from '$lib/components/Onboarding.svelte';
  import UpdateChecker from '$lib/components/UpdateChecker.svelte';
  import SyncStatus from '$lib/components/SyncStatus.svelte';
  import LogTable from '$lib/components/LogTable.svelte';
  import TopNav from '$lib/components/TopNav.svelte';
  import type { LookupBundle, ProductionEventRow } from '$lib/types/codo';

  type BootState =
    | { kind: 'loading'; step: string }
    | { kind: 'needs_onboarding' }
    | {
        kind: 'ready';
        version: string;
        lookups: LookupBundle;
        recent: ProductionEventRow[];
      }
    | { kind: 'error'; message: string };

  let state: BootState = $state({ kind: 'loading', step: 'starting…' });

  async function refresh() {
    state = { kind: 'loading', step: 'starting…' };
    try {
      state = { kind: 'loading', step: 'checking onboarding…' };
      const onboarded = await invoke<boolean>('is_onboarded');
      if (!onboarded) {
        state = { kind: 'needs_onboarding' };
        return;
      }

      state = { kind: 'loading', step: 'opening local database…' };
      await invoke<void>('init_db');

      state = { kind: 'loading', step: 'loading lookups + events…' };
      const [version, lookups, recent] = await Promise.all([
        invoke<string>('current_version'),
        invoke<LookupBundle>('list_lookups'),
        // 755 historical rows fit comfortably in memory; client-side
        // filter/sort/paginate has no latency vs. round-tripping for
        // every filter change. We bump the cap when data growth warrants.
        invoke<ProductionEventRow[]>('list_recent_events', { limit: 10000 })
      ]);
      state = { kind: 'ready', version, lookups, recent };
    } catch (err) {
      state = { kind: 'error', message: String(err) };
    }
  }

  async function refreshRecent() {
    if (state.kind !== 'ready') return;
    const recent = await invoke<ProductionEventRow[]>('list_recent_events', { limit: 10000 });
    state = { ...state, recent };
  }

  function handleSaved(newRows: ProductionEventRow[]) {
    if (state.kind !== 'ready') return;
    state = { ...state, recent: [...newRows, ...state.recent] };
    refreshRecent();
  }

  onMount(refresh);
</script>

{#if state.kind === 'loading'}
  <main class="min-h-screen w-full flex flex-col items-center justify-center gap-3 px-6 py-12">
    <h1 class="text-3xl font-bold tracking-tight">codo</h1>
    <p class="font-mono text-sm text-neutral-600 dark:text-neutral-400">{state.step}</p>
  </main>
{:else if state.kind === 'needs_onboarding'}
  <main class="min-h-screen w-full flex flex-col items-center justify-center gap-6 px-6 py-12">
    <header class="text-center">
      <h1 class="text-4xl font-bold tracking-tight">codo</h1>
      <p class="mt-2 text-sm text-neutral-600 dark:text-neutral-400">
        CI charcoal production log · first-launch setup
      </p>
    </header>
    <Onboarding onComplete={refresh} />
  </main>
{:else if state.kind === 'ready'}
  <TopNav version={state.version} />
  <main class="min-h-screen w-full flex flex-col gap-3 px-4 py-3">
    <!-- Unified log: input row IS the next row visually -->
    <LogTable lookups={state.lookups} rows={state.recent} onSaved={handleSaved} />

    <!-- Maintenance: cloud backup (sync) + app updates -->
    <details class="text-xs">
      <summary class="cursor-pointer px-1 text-neutral-500 hover:text-neutral-700 dark:hover:text-neutral-300">
        app maintenance
      </summary>
      <div class="mt-2 space-y-2">
        <SyncStatus />
        <UpdateChecker />
      </div>
    </details>
  </main>
{:else}
  <main class="min-h-screen w-full flex flex-col items-center justify-center gap-6 px-6 py-12">
    <header class="text-center">
      <h1 class="text-4xl font-bold tracking-tight">codo</h1>
    </header>
    <section class="w-full max-w-xl rounded-xl border border-red-300 bg-red-50 p-6 space-y-3 dark:border-red-900 dark:bg-red-950">
      <h2 class="text-lg font-semibold text-red-700 dark:text-red-200">Boot error</h2>
      <pre class="text-xs whitespace-pre-wrap text-red-700 dark:text-red-300">{state.message}</pre>
      <button
        class="rounded-md px-3 py-1.5 text-sm bg-neutral-200 hover:bg-neutral-300 text-neutral-900 dark:bg-neutral-800 dark:hover:bg-neutral-700 dark:text-neutral-100"
        onclick={refresh}
      >
        Retry
      </button>
    </section>
  </main>
{/if}
