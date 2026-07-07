import { defineConfig } from "vite";

// Config Vite pour l'app Tauri v2 (front TS minimal, multi-fenetres : popover + analytics stub).
// Port fixe requis par Tauri (devUrl dans tauri.conf.json).
export default defineConfig({
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    target: "es2021",
    // Deux points d'entree : la popover (index) et la fenetre stub analytics.
    rollupOptions: {
      input: {
        main: "index.html",
        analytics: "analytics.html",
      },
    },
  },
});
