<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import Onboarding from '$lib/components/Onboarding.svelte';
  import UpdateChecker from '$lib/components/UpdateChecker.svelte';
  import LogTable from '$lib/components/LogTable.svelte';
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
    // Resume-friendly boot: every step is idempotent so codo recovers from
    // any partial-onboarding state (creds in keyring but migrations not run,
    // etc.).
    state = { kind: 'loading', step: 'starting…' };
    try {
      state = { kind: 'loading', step: 'checking onboarding…' };
      const onboarded = await invoke<boolean>('is_onboarded');
      if (!onboarded) {
        state = { kind: 'needs_onboarding' };
        return;
      }

      state = { kind: 'loading', step: 'applying migrations to remote DB…' };
      await invoke<void>('run_remote_migrations');

      state = { kind: 'loading', step: 'opening connection…' };
      await invoke<void>('init_local_replica');

      state = { kind: 'loading', step: 'loading lookups + recent events…' };
      const [version, lookups, recent] = await Promise.all([
        invoke<string>('current_version'),
        invoke<LookupBundle>('list_lookups'),
        invoke<ProductionEventRow[]>('list_recent_events', { limit: 20 })
      ]);
      state = { kind: 'ready', version, lookups, recent };
    } catch (err) {
      state = { kind: 'error', message: String(err) };
    }
  }

  async function refreshRecent() {
    if (state.kind !== 'ready') return;
    const recent = await invoke<ProductionEventRow[]>('list_recent_events', { limit: 20 });
    state = { ...state, recent };
  }

  function handleSaved(newRows: ProductionEventRow[]) {
    // Optimistic prepend; refreshRecent in the background will re-pull.
    if (state.kind !== 'ready') return;
    state = { ...state, recent: [...newRows, ...state.recent].slice(0, 20) };
    refreshRecent();
  }

  onMount(refresh);
</script>

{#if state.kind === 'loading'}
  <main class="min-h-screen w-full flex flex-col items-center justify-center gap-3 px-6 py-12">
    <h1 class="text-3xl font-bold tracking-tight">codo</h1>
    <p class="text-neutral-400 font-mono text-sm">{state.step}</p>
  </main>
{:else if state.kind === 'needs_onboarding'}
  <main class="min-h-screen w-full flex flex-col items-center justify-center gap-6 px-6 py-12">
    <header class="text-center">
      <h1 class="text-4xl font-bold tracking-tight">codo</h1>
      <p class="mt-2 text-sm text-neutral-400">
        CI charcoal production log · first-launch setup
      </p>
    </header>
    <Onboarding onComplete={refresh} />
  </main>
{:else if state.kind === 'ready'}
  <main class="min-h-screen w-full flex flex-col gap-3 px-4 py-3">
    <!-- Top bar -->
    <header class="flex items-baseline justify-between gap-3 px-1">
      <div class="flex items-baseline gap-3">
        <h1 class="text-xl font-bold tracking-tight">codo</h1>
        <span class="text-xs font-mono text-neutral-500">v{state.version}</span>
        <span class="text-xs text-neutral-600">·</span>
        <span class="text-xs text-emerald-400/70 font-mono">connected</span>
      </div>
      <div class="text-xs text-neutral-600 font-mono">CI Cebu · Step 3 vertical slice</div>
    </header>

    <!-- Unified log: input row IS the next row visually -->
    <LogTable lookups={state.lookups} rows={state.recent} onSaved={handleSaved} />

    <!-- Updater (small panel at the bottom) -->
    <details class="text-xs">
      <summary class="cursor-pointer text-neutral-500 hover:text-neutral-300 px-1">
        app maintenance
      </summary>
      <div class="mt-2">
        <UpdateChecker />
      </div>
    </details>
  </main>
{:else}
  <main class="min-h-screen w-full flex flex-col items-center justify-center gap-6 px-6 py-12">
    <header class="text-center">
      <h1 class="text-4xl font-bold tracking-tight">codo</h1>
    </header>
    <section class="w-full max-w-xl rounded-xl border border-red-900 bg-red-950 p-6 space-y-3">
      <h2 class="text-lg font-semibold text-red-200">Boot error</h2>
      <pre class="text-xs text-red-300 whitespace-pre-wrap">{state.message}</pre>
      <button
        class="rounded-md bg-neutral-800 px-3 py-1.5 text-sm hover:bg-neutral-700"
        onclick={refresh}
      >
        Retry
      </button>
    </section>
  </main>
{/if}
