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
  // Every run takes a slot in the machine-wide browser-test queue (TLV-10).
  globalSetup: "./e2e/_liveness/queue-setup.ts",
  globalTeardown: "./e2e/_liveness/queue-teardown.ts",
  // The test server is declared here, never in `webServer`: the queue adopts the
  // preview already answering on 14206, or starts it detached so that no run
  // owns it (testing-runtime TLV-11, 12, DEC-745). `--outDir .` makes preview
  // serve the widget project ROOT statically (not just dist/), so
  // `/e2e/fixture.html` and the fixture's `../dist/widget.js` both resolve --
  // and the bundle is served as the real minified artifact, not a
  // dev-transformed module. (Bare `vite preview` serves only dist/.)
  metadata: {
    testServer: {
      url: `http://localhost:${PORT}/e2e/fixture.html`,
      command: ["npx", "--no-install", "vite", "preview", "--outDir", ".", "--port", String(PORT), "--strictPort"],
    },
  },
  // No time limit stops a test, an assertion or an action (DEC-743): a busy
  // machine making a run slow is not a failure. `0` means none here.
  timeout: 0,
  expect: { timeout: 0 },
  use: {
    actionTimeout: 0,
    navigationTimeout: 0,
    baseURL: `http://localhost:${PORT}`,
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
