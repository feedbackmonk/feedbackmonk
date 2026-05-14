/// <reference types="vitest" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Dev port 14204 is reserved for feedbackmonk admin UI in
// ~/.claude/MACHINE_CONFIG.md Dev Port Registry. strictPort: true MUST stay
// — the 2026-04-26 SessionHelm/WinLocksmith incident showed that silent
// fallback to the next free port renders one project's frontend inside
// another's WebView. Fail loud instead.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 14204,
    strictPort: true,
    proxy: {
      "/api": {
        target: "http://localhost:14304",
        changeOrigin: false,
      },
    },
  },
  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    css: false,
    // Vitest picks up *.test.* and *.spec.* by default; e2e/ is Playwright's
    // turf and uses a different `test` runtime that conflicts at import.
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    exclude: ["e2e/**", "node_modules/**", "dist/**"],
  },
});
