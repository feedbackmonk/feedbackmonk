import { defineConfig, devices } from "@playwright/test";

const PORT = 14204;

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
  // dev server already answering on the reserved port, or starts it detached so
  // that no run owns it (testing-runtime TLV-11, 12, DEC-745). A run in a
  // worktree therefore uses whichever tree's dev server holds 14204.
  metadata: {
    testServer: { url: `http://localhost:${PORT}`, command: ["npm", "run", "dev"] },
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
