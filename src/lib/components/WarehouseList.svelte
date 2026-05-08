<script lang="ts">
  import type { WarehouseSummary } from '$lib/types/codo';

  type Props = { summaries: WarehouseSummary[] };
  let { summaries }: Props = $props();

  function fmtDate(iso: string | null): string {
    if (!iso) return 'never';
    return iso;
  }
</script>

<div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
  {#each summaries as w (w.code)}
    {@const isDvo = w.default_unit === 'kg'}
    {@const href = isDvo ? '/warehouses?dvo=1' : `/warehouses?w=${encodeURIComponent(w.code)}`}
    <a
      {href}
      class="block rounded-md border p-4 transition border-neutral-300 bg-white hover:border-emerald-500 hover:bg-emerald-50/40 dark:border-neutral-800 dark:bg-neutral-950 dark:hover:border-emerald-600 dark:hover:bg-emerald-950/30"
      data-sveltekit-noscroll
    >
      <div class="flex items-baseline justify-between mb-2">
        <h3 class="text-lg font-bold tracking-tight">{w.code}</h3>
        <span
          class="text-[10px] uppercase tracking-wider font-semibold px-1.5 py-0.5 rounded {isDvo
            ? 'bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-300'
            : 'bg-emerald-100 text-emerald-800 dark:bg-emerald-900/30 dark:text-emerald-300'}"
        >
          {isDvo ? 'kg / DVO batches' : 'flec count'}
        </span>
      </div>

      {#if isDvo}
        <p class="text-sm text-neutral-600 dark:text-neutral-400 mb-3">
          Davao container product, tracked per batch.
        </p>
      {:else}
        <div class="mb-3">
          <p class="text-[10px] uppercase tracking-wide text-neutral-500">Total flec on hand</p>
          <p class="text-2xl font-bold font-mono">
            {w.total_flec ?? '—'}
          </p>
        </div>
      {/if}

      <div class="grid grid-cols-2 gap-2 text-xs font-mono">
        <div>
          <p class="text-[10px] uppercase tracking-wide text-neutral-500">Events</p>
          <p>{w.event_count}</p>
        </div>
        <div>
          <p class="text-[10px] uppercase tracking-wide text-neutral-500">Last activity</p>
          <p>{fmtDate(w.last_event_date)}</p>
        </div>
      </div>
    </a>
  {/each}
</div>
