import { defineConfig, devices } from "@playwright/test";

const PORT = 14204;

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
    timeout: 60_000,
  },
});
