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
      progress = 'opening connection + seeding lookups…';
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

<section class="w-full max-w-xl rounded-xl border border-neutral-300 bg-neutral-50 p-6 space-y-4 dark:border-neutral-800 dark:bg-neutral-900">
  <header>
    <h2 class="text-lg font-semibold text-neutral-900 dark:text-neutral-100">
      First-launch setup
    </h2>
    <p class="text-sm text-neutral-700 dark:text-neutral-400">
      Paste a Turso Platform API token. codo creates a database in your account, stores the
      DB-level URL + token in a chmod-600 file under your app-data dir, and discards the
      Platform token immediately after.
      See <code class="text-xs font-mono">PROJECT_BRAIN.md §5 #12</code> for the rationale.
    </p>
  </header>

  <label class="block space-y-1">
    <span class="text-xs uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500">
      Platform API token
    </span>
    <input
      type="password"
      class="w-full rounded-md border border-neutral-300 bg-white px-3 py-2 text-sm font-mono text-neutral-900 focus:border-emerald-500 focus:outline-none dark:border-neutral-800 dark:bg-neutral-950 dark:text-neutral-100 dark:focus:border-emerald-600"
      placeholder="eyJ…"
      bind:value={platformToken}
      disabled={busy}
    />
  </label>

  <label class="block space-y-1">
    <span class="text-xs uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500">
      Database name
    </span>
    <input
      type="text"
      class="w-full rounded-md border border-neutral-300 bg-white px-3 py-2 text-sm font-mono text-neutral-900 focus:border-emerald-500 focus:outline-none dark:border-neutral-800 dark:bg-neutral-950 dark:text-neutral-100 dark:focus:border-emerald-600"
      bind:value={dbName}
      disabled={busy}
    />
  </label>

  {#if progress}
    <p class="text-sm font-mono text-neutral-700 dark:text-neutral-400">{progress}</p>
  {/if}
  {#if err}
    <pre class="text-sm font-mono text-red-700 whitespace-pre-wrap dark:text-red-300">{err}</pre>
  {/if}

  <button
    class="rounded-md px-4 py-2 text-sm font-medium bg-emerald-600 hover:bg-emerald-500 text-white disabled:opacity-50 dark:bg-neutral-100 dark:text-neutral-900 dark:hover:bg-white"
    onclick={submit}
    disabled={busy || !platformToken.trim()}
  >
    {busy ? 'working…' : 'Create database & continue'}
  </button>
</section>
