import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';
import tailwindcss from '@tailwindcss/vite';

// Vite configuration for the Noob-Q SPA.
//
// Production: `vite build` writes `dist/`, which the Rust side serves from
// disk (the standalone binary, `assets_dir`) or embeds in the plug-in
// binary (`include_dir!` under `--features plugin`). Build the SPA before
// building the plug-in, or `include_dir!` has nothing to embed.
//
// Development: `vite` hot-reloads the SPA and proxies the WebSocket and the
// discovery endpoints to a running vst3-web-stratum server (`VST3_WEB_STRATUM_PORT`, default
// 4242, the standalone's preferred port; if that port was taken the
// standalone moved up and printed the port it got):
//
//     cargo run -p noob-q --bin noob-q-standalone      # terminal 1
//     VST3_WEB_STRATUM_PORT=4242 npm run dev                     # terminal 2, in web/
const serverPort = process.env.VST3_WEB_STRATUM_PORT || '4242';
const repoRoot = fileURLToPath(new URL('../../../', import.meta.url));

export default defineConfig({
  plugins: [vue(), tailwindcss()],
  // Relative asset URLs: the page must work from whatever origin and port
  // the plug-in's server ends up on, and from an embedded web view.
  base: './',
  resolve: {
    // `@elyerinfox/vst3-web-stratum` is a `file:../../../crates/vst3-web-stratum/web` dependency, so npm links it
    // from the repo root. Keeping the symlink makes Vite treat the library
    // as part of this project (Tailwind scans it, HMR follows edits to it)
    // and `dedupe` guarantees the library's Vue layer and this app share one
    // copy of `vue`, which reactivity requires.
    preserveSymlinks: true,
    dedupe: ['vue'],
  },
  // The framework is linked from the repository (file:); keep it out of the
  // dependency pre-bundle so edits to it hot-reload instead of being frozen
  // into node_modules/.vite at start-up.
  optimizeDeps: { exclude: ['@elyerinfox/vst3-web-stratum'] },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    // The page only ever runs in a current WebView2 / WebKit / Chromium.
    target: 'es2022',
    // The plug-in embeds `dist/` byte for byte; keep it small.
    sourcemap: false,
  },
  server: {
    port: 5173,
    strictPort: false,
    // The linked library lives outside this directory; allow serving it.
    fs: { allow: [repoRoot] },
    proxy: {
      // The plug-in's WebSocket, so the page can talk to the real DSP.
      '/ws': { target: `ws://127.0.0.1:${serverPort}`, ws: true },
      // `/instance` and `/instances` (prefix match), used by the instance menu.
      '/instance': { target: `http://127.0.0.1:${serverPort}` },
    },
  },
});
