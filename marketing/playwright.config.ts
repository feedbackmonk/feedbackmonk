import { defineConfig, devices } from "@playwright/test";

// Marketing-site a11y harness. Builds the static site once, serves via
// `astro preview` on the reserved Dev Port Registry slot 14210, then runs
// the axe-core smoke spec across every page in the C23 routing scheme.

const PORT = 14210;

export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: 1,
  reporter: [["list"]],
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
  webServer: {
    // Mirrors admin-ui/playwright.config.ts: direct path to the resolved
    // binary because Playwright's webServer spawn on Windows does not
    // inherit a shell, so .cmd extension resolution doesn't happen.
    command:
      process.platform === "win32"
        ? "node_modules\\.bin\\astro.cmd preview --port 14210 --host 127.0.0.1"
        : "node_modules/.bin/astro preview --port 14210 --host 127.0.0.1",
    url: `http://127.0.0.1:${PORT}/`,
    reuseExistingServer: !process.env.CI,
    // time-bound: no limit, death still seen at once — webServer has no
    // unlimited value (`0` falls back to 60 s, `Infinity` fires at once), and
    // its launch wait races the server process, so a build that dies fails now.
    timeout: 2_147_483_647,
  },
});
