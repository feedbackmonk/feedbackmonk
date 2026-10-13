import { defineConfig, devices } from "@playwright/test";

const PORT = 14204;

export default defineConfig({
  testDir: "./e2e",
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
    // Direct path to `vite.cmd` (not `npm run dev` or `npx vite`) —
    // Playwright's webServer spawn on Windows does NOT inherit a shell,
    // so .cmd extension resolution doesn't happen. Pointing at the
    // resolved binary avoids `'vite' is not recognized` failures.
    command:
      process.platform === "win32"
        ? "node_modules\\.bin\\vite.cmd"
        : "node_modules/.bin/vite",
    url: `http://localhost:${PORT}`,
    reuseExistingServer: !process.env.CI,
    // time-bound: no limit, death still seen at once — webServer has no
    // unlimited value (`0` falls back to 60 s, `Infinity` fires at once), and
    // its launch wait races the server process, so a build that dies fails now.
    timeout: 2_147_483_647,
  },
});
