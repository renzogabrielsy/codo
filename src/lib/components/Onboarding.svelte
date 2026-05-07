<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  type Props = { onComplete: () => void };
  let { onComplete }: Props = $props();

  let platformToken = $state('');
  let dbName = $state('codo');
  let busy = $state(false);
  let err: string | null = $state(null);
  let progress: string | null = $state(null);

  async function submit() {
    err = null;
    progress = 'creating database via Turso Platform API…';
    busy = true;
    try {
      await invoke<void>('onboard_turso', {
        platformToken: platformToken.trim(),
        dbName: dbName.trim() || 'codo'
      });
      progress = 'applying migrations to remote DB…';
      await invoke<void>('run_remote_migrations');
      progress = 'opening local replica + seeding lookups…';
      await invoke<void>('init_local_replica');
      progress = 'done.';
      onComplete();
    } catch (e) {
      err = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<section class="w-full max-w-xl rounded-xl border border-neutral-800 bg-neutral-900 p-6 space-y-4">
  <header>
    <h2 class="text-lg font-semibold">First-launch setup</h2>
    <p class="text-sm text-neutral-400">
      Paste a Turso Platform API token. codo creates a database in your account, stores the
      DB-level URL + token in the OS keyring, and discards the Platform token immediately after.
      See <code class="text-xs">PROJECT_BRAIN.md §5 #12</code> for the rationale.
    </p>
  </header>

  <label class="block space-y-1">
    <span class="text-xs uppercase tracking-wide text-neutral-500">Platform API token</span>
    <input
      type="password"
      class="w-full rounded-md bg-neutral-950 border border-neutral-800 px-3 py-2 text-sm font-mono focus:outline-none focus:border-neutral-600"
      placeholder="eyJ…"
      bind:value={platformToken}
      disabled={busy}
    />
  </label>

  <label class="block space-y-1">
    <span class="text-xs uppercase tracking-wide text-neutral-500">Database name</span>
    <input
      type="text"
      class="w-full rounded-md bg-neutral-950 border border-neutral-800 px-3 py-2 text-sm font-mono focus:outline-none focus:border-neutral-600"
      bind:value={dbName}
      disabled={busy}
    />
  </label>

  {#if progress}
    <p class="text-xs text-neutral-400 font-mono">{progress}</p>
  {/if}
  {#if err}
    <pre class="text-xs text-red-300 whitespace-pre-wrap">{err}</pre>
  {/if}

  <button
    class="rounded-md bg-neutral-100 text-neutral-900 px-4 py-2 text-sm font-medium hover:bg-white disabled:opacity-50"
    onclick={submit}
    disabled={busy || !platformToken.trim()}
  >
    {busy ? 'working…' : 'Create database & continue'}
  </button>
</section>
