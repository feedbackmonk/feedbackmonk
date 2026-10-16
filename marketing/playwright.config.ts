import { defineConfig, devices } from "@playwright/test";

// Marketing-site a11y harness. Builds the static site once, serves via
// `astro preview` on the reserved Dev Port Registry slot 14222, then runs
// the axe-core smoke spec across every page in the C23 routing scheme.

const PORT = 14222;

export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: 1,
  reporter: [["list"]],
  // Every run takes a slot in the machine-wide browser-test queue (TLV-10).
  globalSetup: "./tests/_liveness/queue-setup.ts",
  globalTeardown: "./tests/_liveness/queue-teardown.ts",
  // The test server is declared here, never in `webServer`: the queue adopts the
  // preview already answering on the port, or starts it detached so that no run
  // owns it (testing-runtime TLV-11, 12, DEC-745).
  metadata: {
    testServer: { url: `http://127.0.0.1:${PORT}/`, command: ["npm", "run", "preview"] },
  },
  // No time limit stops a test, an assertion or an action (DEC-743): a busy
  // machine making a run slow is not a failure. `0` means none here.
  timeout: 0,
  expect: { timeout: 0 },
  use: {
    actionTimeout: 0,
    navigationTimeout: 0,
    baseURL: `http://127.0.0.1:${PORT}`,
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
