<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { check, type Update } from '@tauri-apps/plugin-updater';
  import { relaunch } from '@tauri-apps/plugin-process';

  // Per PROJECT_BRAIN.md §4.7: surface every input to the decision so it's
  // cross-checkable on the same screen — current version, available version,
  // bytes downloaded, total size, the endpoint that was queried.
  type State =
    | { kind: 'idle' }
    | { kind: 'checking' }
    | { kind: 'no_update'; current: string; checked_at: Date }
    | {
        kind: 'available';
        current: string;
        available: string;
        notes: string | null;
        update: Update;
      }
    | {
        kind: 'downloading';
        current: string;
        available: string;
        downloaded: number;
        total: number | null;
      }
    | { kind: 'installed'; current: string; available: string }
    | { kind: 'error'; message: string };

  let state: State = $state({ kind: 'idle' });

  async function checkNow() {
    state = { kind: 'checking' };
    try {
      const current = await invoke<string>('current_version');
      const update = await check();
      if (!update?.available) {
        state = { kind: 'no_update', current, checked_at: new Date() };
        return;
      }
      state = {
        kind: 'available',
        current,
        available: update.version,
        notes: update.body ?? null,
        update
      };
    } catch (e) {
      state = { kind: 'error', message: String(e) };
    }
  }

  async function downloadAndInstall() {
    if (state.kind !== 'available') return;
    const { current, available, update } = state;
    state = {
      kind: 'downloading',
      current,
      available,
      downloaded: 0,
      total: null
    };
    try {
      let downloaded = 0;
      let total: number | null = null;
      await update.downloadAndInstall((event) => {
        if (event.event === 'Started') {
          total = event.data.contentLength ?? null;
          state = { kind: 'downloading', current, available, downloaded: 0, total };
        } else if (event.event === 'Progress') {
          downloaded += event.data.chunkLength;
          state = { kind: 'downloading', current, available, downloaded, total };
        } else if (event.event === 'Finished') {
          state = { kind: 'installed', current, available };
        }
      });
      state = { kind: 'installed', current, available };
      // Pause briefly so the user sees the "installed" state, then relaunch.
      setTimeout(async () => {
        await relaunch();
      }, 1200);
    } catch (e) {
      state = { kind: 'error', message: String(e) };
    }
  }

  function formatBytes(n: number): string {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${(n / 1024 / 1024).toFixed(2)} MB`;
  }

  function progressPct(downloaded: number, total: number | null): string {
    if (!total || total <= 0) return '—';
    return `${((downloaded / total) * 100).toFixed(0)}%`;
  }
</script>

<section class="rounded-md border border-neutral-800 bg-neutral-950 p-3 space-y-2">
  <div class="flex items-center justify-between gap-3">
    <div>
      <p class="text-xs uppercase tracking-wide text-neutral-500">Updates</p>
      <p class="text-xs text-neutral-500 font-mono">
        endpoint: github releases → latest.json
      </p>
    </div>
    <button
      class="rounded-md bg-neutral-800 hover:bg-neutral-700 px-3 py-1.5 text-xs font-medium disabled:opacity-50"
      onclick={checkNow}
      disabled={state.kind === 'checking' || state.kind === 'downloading'}
    >
      {state.kind === 'checking' ? 'checking…' : 'Check for updates'}
    </button>
  </div>

  {#if state.kind === 'idle'}
    <p class="text-xs text-neutral-500">
      Click <em>Check for updates</em> to query the GitHub Releases endpoint.
    </p>
  {:else if state.kind === 'checking'}
    <p class="text-xs text-neutral-400 font-mono">querying github…</p>
  {:else if state.kind === 'no_update'}
    <p class="text-xs text-neutral-300 font-mono">
      v{state.current} is current.
      <span class="text-neutral-500">
        (checked {state.checked_at.toLocaleTimeString()})
      </span>
    </p>
  {:else if state.kind === 'available'}
    <div class="space-y-2">
      <p class="text-sm font-mono">
        <span class="text-neutral-400">{state.current}</span>
        <span class="text-neutral-600">→</span>
        <span class="text-emerald-300">{state.available}</span>
      </p>
      {#if state.notes}
        <details class="text-xs text-neutral-400">
          <summary class="cursor-pointer hover:text-neutral-200">release notes</summary>
          <pre class="mt-1 whitespace-pre-wrap font-mono text-[11px] leading-tight">{state.notes}</pre>
        </details>
      {/if}
      <button
        class="rounded-md bg-emerald-700 hover:bg-emerald-600 px-3 py-1.5 text-xs font-medium"
        onclick={downloadAndInstall}
      >
        Download & install
      </button>
    </div>
  {:else if state.kind === 'downloading'}
    <div class="space-y-1 font-mono text-xs">
      <p class="text-neutral-300">
        downloading {state.current} <span class="text-neutral-600">→</span> {state.available}
      </p>
      <p class="text-neutral-500">
        {formatBytes(state.downloaded)}{#if state.total} / {formatBytes(state.total)} ({progressPct(
            state.downloaded,
            state.total
          )}){/if}
      </p>
      {#if state.total}
        <div class="h-1 w-full bg-neutral-800 rounded overflow-hidden">
          <div
            class="h-full bg-emerald-500 transition-[width] duration-300"
            style="width: {progressPct(state.downloaded, state.total)}"
          ></div>
        </div>
      {/if}
    </div>
  {:else if state.kind === 'installed'}
    <p class="text-xs text-emerald-300 font-mono">
      installed v{state.available} — relaunching…
    </p>
  {:else if state.kind === 'error'}
    <pre class="text-xs text-red-300 whitespace-pre-wrap font-mono">{state.message}</pre>
  {/if}
</section>
