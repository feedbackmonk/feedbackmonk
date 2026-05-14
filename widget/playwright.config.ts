import { defineConfig, devices } from "@playwright/test";

// feedbackmonk widget Playwright config.
//
// Uses vite preview server on port 14206 (deconflicted from admin-ui's 14204
// dev / 14205 e2e preview and feedbackmonk-api's 14304 — see
// ~/.claude/MACHINE_CONFIG.md Dev Port Registry; 14206 is the widget e2e port).
//
// `strictPort: true` is required per DEC-FBR-IMPL-04 — silent port collision
// would render one project's preview inside another's window context.

const PORT = 14206;

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: 1,
  reporter: [["list"]],
  use: {
    baseURL: `http://localhost:${PORT}`,
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    command: `vite preview --port ${PORT} --strictPort`,
    url: `http://localhost:${PORT}/e2e/fixture.html`,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
