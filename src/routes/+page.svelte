<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import Onboarding from '$lib/components/Onboarding.svelte';
  import UpdateChecker from '$lib/components/UpdateChecker.svelte';

  type BootState =
    | { kind: 'loading'; step: string }
    | { kind: 'needs_onboarding' }
    | { kind: 'ready'; warehouses: { id: number; code: string; default_unit: string }[] }
    | { kind: 'error'; message: string };

  let state: BootState = $state({ kind: 'loading', step: 'starting…' });

  async function refresh() {
    // Resume-friendly boot: onboarding may have left codo in a partially-
    // initialized state (creds in keyring but remote migrations never ran,
    // or local replica never opened). Each step is idempotent — safe to
    // run on every cold start.
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

      state = { kind: 'loading', step: 'opening local replica…' };
      await invoke<void>('init_local_replica');

      state = { kind: 'loading', step: 'loading warehouses…' };
      const warehouses = await invoke<{ id: number; code: string; default_unit: string }[]>(
        'list_warehouses'
      );
      state = { kind: 'ready', warehouses };
    } catch (err) {
      state = { kind: 'error', message: String(err) };
    }
  }

  onMount(refresh);
</script>

<main class="min-h-screen w-full flex flex-col items-center justify-center gap-6 px-6 py-12">
  <header class="text-center">
    <h1 class="text-4xl font-bold tracking-tight">codo</h1>
    <p class="mt-2 text-sm text-neutral-400">
      CI charcoal production log · Step 2 scaffold
    </p>
  </header>

  {#if state.kind === 'loading'}
    <p class="text-neutral-400 font-mono text-sm">{state.step}</p>
  {:else if state.kind === 'needs_onboarding'}
    <Onboarding onComplete={refresh} />
  {:else if state.kind === 'ready'}
    <section class="w-full max-w-xl rounded-xl border border-neutral-800 bg-neutral-900 p-6 space-y-3">
      <h2 class="text-lg font-semibold">Hello, codo.</h2>
      <p class="text-sm text-neutral-400">
        Local replica connected. Migrations applied. Lookup tables seeded.
      </p>
      <div class="rounded-md border border-neutral-800 bg-neutral-950 p-3">
        <p class="mb-2 text-xs uppercase tracking-wide text-neutral-500">Warehouses</p>
        <ul class="text-sm font-mono space-y-1">
          {#each state.warehouses as w}
            <li class="flex justify-between">
              <span>{w.code}</span>
              <span class="text-neutral-500">{w.default_unit}</span>
            </li>
          {/each}
        </ul>
      </div>
      <UpdateChecker />
      <p class="text-xs text-neutral-500">
        Step 3 wires up the &quot;Log a production event&quot; form.
      </p>
    </section>
  {:else}
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
  {/if}
</main>
