import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

// IMPORTANT: SvelteKit plugin FIRST, Tailwind v4 SECOND.
// Reversing the order silently breaks Tailwind class scanning.
// (PROJECT_BRAIN.md §5 gotcha #4)
export default defineConfig(async () => ({
  plugins: [sveltekit(), tailwindcss()],

  // Vite options tailored for Tauri development.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: '127.0.0.1',
    watch: {
      // Tauri creates its own build artifacts; don't trigger HMR on them.
      ignored: ['**/src-tauri/**']
    }
  }
}));
