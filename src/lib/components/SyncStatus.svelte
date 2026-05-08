<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import type { SyncStatus, SyncSummary } from '$lib/types/codo';

  // PROJECT_BRAIN.md §3 + §4.2 (post-2026-05-XX update): codo's primary
  // store is local SQLite. The remote Turso DB is a manual one-way backup.
  // This component shows pending-row count + last-synced timestamp + a
  // Sync now button that pushes dirty rows to the cloud.

  let status: SyncStatus = $state({ last_synced_at: null, pending_count: 0 });
  let busy = $state(false);
  let err: string | null = $state(null);
  let lastResult: SyncSummary | null = $state(null);

  async function refreshStatus() {
    try {
      status = await invoke<SyncStatus>('get_sync_status');
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    }
  }

  async function syncNow() {
    err = null;
    if (busy) return;
    busy = true;
    try {
      lastResult = await invoke<SyncSummary>('sync_now');
      await refreshStatus();
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function fmtRelative(iso: string | null): string {
    if (!iso) return 'never';
    const then = new Date(iso).getTime();
    if (Number.isNaN(then)) return iso;
    const seconds = Math.floor((Date.now() - then) / 1000);
    if (seconds < 60) return `${seconds}s ago`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
    if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
    return `${Math.floor(seconds / 86400)}d ago`;
  }

  function fmtAbsolute(iso: string | null): string {
    if (!iso) return '';
    return new Date(iso).toLocaleString();
  }

  onMount(refreshStatus);
</script>

<section
  class="rounded-md border p-3 space-y-2 border-neutral-300 bg-white dark:border-neutral-800 dark:bg-neutral-950"
>
  <div class="flex items-center justify-between gap-3">
    <div>
      <p class="text-xs uppercase tracking-wide font-semibold text-neutral-700 dark:text-neutral-500">
        Cloud backup
      </p>
      <p class="text-xs font-mono text-neutral-500">
        local-first · Turso DB is a one-way backup target
      </p>
    </div>
    <button
      type="button"
      onclick={syncNow}
      disabled={busy}
      class="rounded-md px-3 py-1.5 text-sm font-medium disabled:opacity-50 bg-emerald-600 hover:bg-emerald-500 text-white dark:bg-emerald-700 dark:hover:bg-emerald-600"
    >
      {busy ? 'syncing…' : 'Sync now'}
    </button>
  </div>

  <div class="grid grid-cols-2 gap-2 text-sm">
    <div class="rounded bg-neutral-50 dark:bg-neutral-900/50 px-2 py-1.5">
      <p class="text-[11px] uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500">
        Pending
      </p>
      <p
        class="font-mono {status.pending_count === 0
          ? 'text-emerald-700 dark:text-emerald-400'
          : 'text-amber-700 dark:text-amber-400 font-semibold'}"
      >
        {status.pending_count} {status.pending_count === 1 ? 'row' : 'rows'}
      </p>
    </div>
    <div class="rounded bg-neutral-50 dark:bg-neutral-900/50 px-2 py-1.5">
      <p class="text-[11px] uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500">
        Last synced
      </p>
      <p
        class="font-mono text-neutral-800 dark:text-neutral-300"
        title={fmtAbsolute(status.last_synced_at)}
      >
        {fmtRelative(status.last_synced_at)}
      </p>
    </div>
  </div>

  {#if lastResult && !err}
    <p class="text-xs font-mono text-emerald-700 dark:text-emerald-400">
      ✓ pushed {lastResult.pushed} {lastResult.pushed === 1 ? 'row' : 'rows'} at {fmtAbsolute(
        lastResult.last_synced_at
      )}
    </p>
  {/if}
  {#if err}
    <pre class="text-sm font-mono text-red-700 whitespace-pre-wrap dark:text-red-300">{err}</pre>
  {/if}
</section>
