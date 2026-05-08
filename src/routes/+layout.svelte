<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { setContext } from 'svelte';
  import { writable } from 'svelte/store';

  let { children } = $props();

  // Theme management. Class-based — `dark` on <html> when dark mode is
  // active, absent for light. Persisted to localStorage; falls back to
  // system preference on first launch.
  type Theme = 'dark' | 'light';
  const theme = writable<Theme>('dark');

  function applyTheme(t: Theme) {
    if (typeof document === 'undefined') return;
    document.documentElement.classList.toggle('dark', t === 'dark');
  }

  onMount(() => {
    const stored = localStorage.getItem('codo-theme') as Theme | null;
    const systemDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    const initial: Theme = stored ?? (systemDark ? 'dark' : 'light');
    theme.set(initial);
    applyTheme(initial);

    return theme.subscribe((t) => {
      localStorage.setItem('codo-theme', t);
      applyTheme(t);
    });
  });

  setContext('codo-theme', theme);
</script>

{@render children()}
