<script lang="ts">
  import { page } from '$app/state';
  import { getContext } from 'svelte';
  import type { Writable } from 'svelte/store';

  // Theme owned by +layout.svelte; we just flip it.
  const theme = getContext<Writable<'dark' | 'light'>>('codo-theme');
  let isDark = $state(true);
  $effect(() => {
    if (!theme) return;
    return theme.subscribe((t) => {
      isDark = t === 'dark';
    });
  });
  function toggleTheme() {
    if (!theme) return;
    theme.update((t) => (t === 'dark' ? 'light' : 'dark'));
  }

  type Props = { version?: string };
  let { version = '' }: Props = $props();

  const links = [
    { href: '/', label: 'Production' },
    { href: '/warehouses', label: 'Warehouses' }
  ];
  function isActive(href: string): boolean {
    if (href === '/') return page.url.pathname === '/';
    return page.url.pathname.startsWith(href);
  }
</script>

<header class="flex items-baseline justify-between gap-3 px-4 py-2 border-b border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-950">
  <div class="flex items-baseline gap-3">
    <h1 class="text-xl font-bold tracking-tight">codo</h1>
    {#if version}
      <span class="text-xs font-mono text-neutral-500">v{version}</span>
    {/if}
    <nav class="flex items-center gap-1 ml-3">
      {#each links as l (l.href)}
        <a
          href={l.href}
          class="rounded px-2 py-1 text-sm font-medium transition {isActive(l.href)
            ? 'bg-emerald-600 text-white dark:bg-emerald-500 dark:text-neutral-950'
            : 'text-neutral-700 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800'}"
          data-sveltekit-noscroll
        >
          {l.label}
        </a>
      {/each}
    </nav>
  </div>
  <div class="flex items-center gap-3">
    <button
      type="button"
      onclick={toggleTheme}
      title={isDark ? 'switch to light mode' : 'switch to dark mode'}
      class="rounded border border-neutral-300 hover:border-neutral-400 dark:border-neutral-700 dark:hover:border-neutral-500 px-2 py-0.5 text-xs font-mono text-neutral-700 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800"
    >
      {isDark ? '☀ light' : '🌙 dark'}
    </button>
  </div>
</header>
