<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Chart, registerables } from 'chart.js/auto';
  import type { ProductionEventRow } from '$lib/types/codo';

  Chart.register(...registerables);

  type Props = { rows: ProductionEventRow[] };
  let { rows }: Props = $props();

  let dailyCanvas: HTMLCanvasElement;
  let dispositionCanvas: HTMLCanvasElement;
  let sourceCanvas: HTMLCanvasElement;
  let gradeCanvas: HTMLCanvasElement;

  let dailyChart: Chart | null = null;
  let dispositionChart: Chart | null = null;
  let sourceChart: Chart | null = null;
  let gradeChart: Chart | null = null;

  // Range filter — affects every chart in this panel.
  let rangeKey = $state<'7d' | '30d' | '90d' | 'all'>('30d');

  function fmtDate(iso: string): string {
    const m = iso.match(/^(\d{4})-(\d{2})-(\d{2})$/);
    if (!m) return iso;
    return `${parseInt(m[2], 10)}/${parseInt(m[3], 10)}`;
  }

  function disp(r: ProductionEventRow): string {
    if (r.disposition_kind === 'flec_bagging') return 'FLEC';
    return r.partner_equipment_code ?? '?';
  }

  function isDark(): boolean {
    return typeof document !== 'undefined' && document.documentElement.classList.contains('dark');
  }
  function gridColor() {
    return isDark() ? 'rgba(115, 115, 115, 0.2)' : 'rgba(115, 115, 115, 0.15)';
  }
  function tickColor() {
    return isDark() ? '#a3a3a3' : '#525252';
  }

  // Filter rows by range.
  const filteredRows = $derived.by(() => {
    if (rangeKey === 'all') return rows;
    const days = rangeKey === '7d' ? 7 : rangeKey === '30d' ? 30 : 90;
    const cutoff = new Date();
    cutoff.setDate(cutoff.getDate() - days);
    const cutoffIso = cutoff.toISOString().slice(0, 10);
    return rows.filter((r) => r.recv_date >= cutoffIso);
  });

  // Daily kg by direction (in/out).
  const dailySeries = $derived.by(() => {
    const dates = new Map<string, { in: number; out: number }>();
    for (const r of filteredRows) {
      const day = r.recv_date;
      const slot = dates.get(day) ?? { in: 0, out: 0 };
      if (r.disposition_kind === 'flec_bagging') slot.in += r.weight_kg;
      else slot.out += r.weight_kg;
      dates.set(day, slot);
    }
    const sorted = [...dates.entries()].sort(([a], [b]) => a.localeCompare(b));
    return {
      labels: sorted.map(([d]) => fmtDate(d)),
      isoDates: sorted.map(([d]) => d),
      ins: sorted.map(([, v]) => v.in),
      outs: sorted.map(([, v]) => v.out)
    };
  });

  const dispositionSeries = $derived.by(() => {
    const totals = new Map<string, number>();
    for (const r of filteredRows) {
      const k = disp(r);
      totals.set(k, (totals.get(k) ?? 0) + r.weight_kg);
    }
    const sorted = [...totals.entries()].sort(([, a], [, b]) => b - a);
    return {
      labels: sorted.map(([k]) => k),
      values: sorted.map(([, v]) => v)
    };
  });

  const sourceSeries = $derived.by(() => {
    const totals = new Map<string, number>();
    for (const r of filteredRows) {
      const k = r.source_code;
      totals.set(k, (totals.get(k) ?? 0) + r.weight_kg);
    }
    const sorted = [...totals.entries()].sort(([, a], [, b]) => b - a);
    return {
      labels: sorted.map(([k]) => k),
      values: sorted.map(([, v]) => v)
    };
  });

  const gradeSeries = $derived.by(() => {
    const totals = new Map<string, number>();
    for (const r of filteredRows) {
      const k = r.grade_code;
      totals.set(k, (totals.get(k) ?? 0) + r.weight_kg);
    }
    const sorted = [...totals.entries()].sort(([, a], [, b]) => b - a);
    return {
      labels: sorted.map(([k]) => k),
      values: sorted.map(([, v]) => v)
    };
  });

  const totalKg = $derived(filteredRows.reduce((s, r) => s + r.weight_kg, 0));
  const totalEvents = $derived(filteredRows.length);

  // Stable color palettes.
  const dispositionColor = (label: string): string => {
    if (label === 'FLEC') return 'rgba(16, 185, 129, 0.7)'; // emerald
    if (/^C[1-4]$/.test(label)) return 'rgba(56, 189, 248, 0.7)'; // sky
    if (/^RK[1-4]$/.test(label)) return 'rgba(245, 158, 11, 0.7)'; // amber
    return 'rgba(115, 115, 115, 0.7)';
  };
  const sourceColors = [
    '#22c55e', '#06b6d4', '#a855f7', '#f59e0b', '#ef4444', '#14b8a6', '#6366f1', '#ec4899'
  ];
  const gradeColors: Record<string, string> = {
    '3X50': '#a78bfa',
    '2X6': '#34d399',
    '3.5': '#fb923c',
    '4X8': '#f472b6'
  };

  function renderDaily() {
    if (!dailyCanvas) return;
    if (dailyChart) dailyChart.destroy();
    dailyChart = new Chart(dailyCanvas, {
      type: 'bar',
      data: {
        labels: dailySeries.labels,
        datasets: [
          {
            label: 'kg in (FLEC bagging)',
            data: dailySeries.ins,
            backgroundColor: 'rgba(16, 185, 129, 0.7)',
            borderColor: 'rgba(16, 185, 129, 1)',
            borderWidth: 1,
            stack: 'flow'
          },
          {
            label: 'kg out (partner takes)',
            data: dailySeries.outs,
            backgroundColor: 'rgba(168, 85, 247, 0.6)',
            borderColor: 'rgba(168, 85, 247, 1)',
            borderWidth: 1,
            stack: 'flow'
          }
        ]
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        plugins: {
          legend: { labels: { color: tickColor() } }
        },
        scales: {
          x: {
            stacked: true,
            ticks: { color: tickColor(), maxRotation: 0 },
            grid: { color: gridColor() }
          },
          y: {
            stacked: true,
            ticks: { color: tickColor() },
            grid: { color: gridColor() }
          }
        }
      }
    });
  }

  function renderDisposition() {
    if (!dispositionCanvas) return;
    if (dispositionChart) dispositionChart.destroy();
    dispositionChart = new Chart(dispositionCanvas, {
      type: 'doughnut',
      data: {
        labels: dispositionSeries.labels,
        datasets: [
          {
            data: dispositionSeries.values,
            backgroundColor: dispositionSeries.labels.map(dispositionColor),
            borderColor: isDark() ? '#0a0a0a' : '#ffffff',
            borderWidth: 2
          }
        ]
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        plugins: {
          legend: { position: 'right', labels: { color: tickColor(), font: { size: 11 } } }
        }
      }
    });
  }

  function renderSource() {
    if (!sourceCanvas) return;
    if (sourceChart) sourceChart.destroy();
    sourceChart = new Chart(sourceCanvas, {
      type: 'bar',
      data: {
        labels: sourceSeries.labels,
        datasets: [
          {
            label: 'kg by source',
            data: sourceSeries.values,
            backgroundColor: sourceSeries.labels.map(
              (_, i) => sourceColors[i % sourceColors.length]
            ),
            borderWidth: 0
          }
        ]
      },
      options: {
        indexAxis: 'y',
        responsive: true,
        maintainAspectRatio: false,
        plugins: { legend: { display: false } },
        scales: {
          x: { ticks: { color: tickColor() }, grid: { color: gridColor() } },
          y: { ticks: { color: tickColor() }, grid: { color: gridColor() } }
        }
      }
    });
  }

  function renderGrade() {
    if (!gradeCanvas) return;
    if (gradeChart) gradeChart.destroy();
    gradeChart = new Chart(gradeCanvas, {
      type: 'doughnut',
      data: {
        labels: gradeSeries.labels,
        datasets: [
          {
            data: gradeSeries.values,
            backgroundColor: gradeSeries.labels.map((l) => gradeColors[l] ?? '#737373'),
            borderColor: isDark() ? '#0a0a0a' : '#ffffff',
            borderWidth: 2
          }
        ]
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        plugins: {
          legend: { position: 'right', labels: { color: tickColor(), font: { size: 11 } } }
        }
      }
    });
  }

  $effect(() => {
    // Re-render whenever the filtered set or grouped series change.
    void filteredRows.length;
    void dailySeries.labels.length;
    void dispositionSeries.labels.length;
    void sourceSeries.labels.length;
    void gradeSeries.labels.length;
    renderDaily();
    renderDisposition();
    renderSource();
    renderGrade();
  });

  // Re-render on theme flip (dark/light) so axis colors update.
  let lastDark = $state(isDark());
  onMount(() => {
    const obs = new MutationObserver(() => {
      const d = isDark();
      if (d !== lastDark) {
        lastDark = d;
        renderDaily();
        renderDisposition();
        renderSource();
        renderGrade();
      }
    });
    obs.observe(document.documentElement, { attributes: true, attributeFilter: ['class'] });
    return () => obs.disconnect();
  });

  onDestroy(() => {
    dailyChart?.destroy();
    dispositionChart?.destroy();
    sourceChart?.destroy();
    gradeChart?.destroy();
  });
</script>

<section class="rounded-md border border-neutral-300 bg-white dark:border-neutral-800 dark:bg-neutral-950 overflow-hidden">
  <header class="flex items-center justify-between gap-3 px-3 py-1.5 border-b border-neutral-300 dark:border-neutral-800">
    <div class="flex items-center gap-3">
      <h3 class="text-xs uppercase tracking-wide font-semibold text-neutral-700 dark:text-neutral-400">
        Production summary
      </h3>
      <span class="text-xs font-mono text-neutral-500">
        {totalEvents} events · {(totalKg / 1000).toFixed(1)} t total
      </span>
    </div>
    <div class="flex items-center gap-1 text-xs font-mono">
      {#each [
        { k: '7d', label: 'last 7d' },
        { k: '30d', label: 'last 30d' },
        { k: '90d', label: 'last 90d' },
        { k: 'all', label: 'all time' }
      ] as r (r.k)}
        <button
          type="button"
          onclick={() => (rangeKey = r.k as '7d' | '30d' | '90d' | 'all')}
          class="rounded px-2 py-0.5 {rangeKey === r.k
            ? 'bg-emerald-600 dark:bg-emerald-500 text-white dark:text-neutral-950'
            : 'border border-neutral-300 dark:border-neutral-700 text-neutral-700 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800'}"
        >
          {r.label}
        </button>
      {/each}
    </div>
  </header>

  <div class="grid grid-cols-1 lg:grid-cols-2 gap-3 p-3">
    <div>
      <p class="text-[11px] uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500 mb-1">
        Daily volume — kg in vs out
      </p>
      <div class="h-48">
        <canvas bind:this={dailyCanvas}></canvas>
      </div>
    </div>
    <div>
      <p class="text-[11px] uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500 mb-1">
        kg by source
      </p>
      <div class="h-48">
        <canvas bind:this={sourceCanvas}></canvas>
      </div>
    </div>
    <div>
      <p class="text-[11px] uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500 mb-1">
        kg by destination (FLEC / C1-4 / RK1-4)
      </p>
      <div class="h-48">
        <canvas bind:this={dispositionCanvas}></canvas>
      </div>
    </div>
    <div>
      <p class="text-[11px] uppercase tracking-wide font-semibold text-neutral-600 dark:text-neutral-500 mb-1">
        kg by grade
      </p>
      <div class="h-48">
        <canvas bind:this={gradeCanvas}></canvas>
      </div>
    </div>
  </div>
</section>
