<script lang="ts">
  import type { ProductionEventRow } from '$lib/types/codo';

  type Props = {
    rows: ProductionEventRow[];
  };
  let { rows }: Props = $props();

  function shortTag(tag: string): string {
    // Tags are long; show last 18 chars so the suffix (which has the
    // disposition + side) stays visible.
    if (tag.length <= 22) return tag;
    return '…' + tag.slice(-22);
  }

  function fmtKg(kg: number): string {
    if (kg >= 1000) return (kg / 1000).toFixed(1) + 'k';
    return kg.toFixed(0);
  }

  function fmtDate(iso: string | null): string {
    if (!iso) return '';
    // ISO 'YYYY-MM-DD' → 'M/D' (compact, Excel-style)
    const m = iso.match(/^(\d{4})-(\d{2})-(\d{2})$/);
    if (!m) return iso;
    return `${parseInt(m[2], 10)}/${parseInt(m[3], 10)}`;
  }

  function dispositionLabel(row: ProductionEventRow): string {
    if (row.disposition_kind === 'flec_bagging') return 'FLEC';
    return row.partner_equipment_code ?? '?';
  }
</script>

<div class="rounded-md border border-neutral-800 bg-neutral-950 overflow-hidden">
  <div class="flex items-baseline justify-between px-3 py-2 border-b border-neutral-800">
    <h3 class="text-xs uppercase tracking-wide text-neutral-500">Recent events</h3>
    <span class="text-xs text-neutral-600 font-mono">last {rows.length}</span>
  </div>
  <div class="overflow-x-auto">
    <table class="w-full text-xs font-mono">
      <thead class="bg-neutral-900/50 text-neutral-500">
        <tr>
          <th class="px-2 py-1.5 text-left font-normal">RECV</th>
          <th class="px-2 py-1.5 text-left font-normal">PROD</th>
          <th class="px-2 py-1.5 text-left font-normal">BATCH</th>
          <th class="px-2 py-1.5 text-left font-normal">SH</th>
          <th class="px-2 py-1.5 text-left font-normal">GRD</th>
          <th class="px-2 py-1.5 text-left font-normal">SRC</th>
          <th class="px-2 py-1.5 text-left font-normal">PLT</th>
          <th class="px-2 py-1.5 text-left font-normal">WHSE</th>
          <th class="px-2 py-1.5 text-left font-normal">SD</th>
          <th class="px-2 py-1.5 text-right font-normal">WT</th>
          <th class="px-2 py-1.5 text-right font-normal">FLEC</th>
          <th class="px-2 py-1.5 text-left font-normal">DISP</th>
          <th class="px-2 py-1.5 text-left font-normal text-neutral-700">UNIQUE TAG</th>
        </tr>
      </thead>
      <tbody>
        {#if rows.length === 0}
          <tr>
            <td colspan="13" class="px-3 py-6 text-center text-neutral-500">
              no events yet — log one below
            </td>
          </tr>
        {:else}
          {#each rows as row (row.id)}
            <tr class="border-t border-neutral-900 hover:bg-neutral-900/40">
              <td class="px-2 py-1 text-neutral-200">{fmtDate(row.recv_date)}</td>
              <td class="px-2 py-1 text-neutral-400">{fmtDate(row.prod_date)}</td>
              <td class="px-2 py-1 text-neutral-300">{row.batch}</td>
              <td class="px-2 py-1 text-neutral-300">{row.shift_code ?? ''}</td>
              <td class="px-2 py-1 text-neutral-300">{row.grade_code}</td>
              <td class="px-2 py-1 text-neutral-300">{row.source_code}</td>
              <td class="px-2 py-1 text-neutral-400">{row.plant_code ?? ''}</td>
              <td class="px-2 py-1 text-neutral-300">{row.warehouse_code ?? ''}</td>
              <td class="px-2 py-1 text-neutral-300">{row.whse_side ?? ''}</td>
              <td class="px-2 py-1 text-right text-neutral-200">{fmtKg(row.weight_kg)}</td>
              <td class="px-2 py-1 text-right text-neutral-300">{row.flec_count ?? ''}</td>
              <td class="px-2 py-1 text-neutral-300">{dispositionLabel(row)}</td>
              <td
                class="px-2 py-1 text-neutral-600 truncate max-w-[180px]"
                title={row.unique_tag}>{shortTag(row.unique_tag)}</td>
            </tr>
          {/each}
        {/if}
      </tbody>
    </table>
  </div>
</div>
