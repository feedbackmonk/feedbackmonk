import { defineConfig } from "vitest/config";

// Unit tests for the widget (FR-FBR-35). Deliberately separate from
// vite.config.ts: the build config is a LIB build with terser + a fixed
// entry, and none of that applies to a test run.
//
// `jsdom` because the DOM builders in ui.ts are the thing under test — the
// English-rendering snapshot is what proves the catalog extraction changed no
// visible text, and it can only be taken against a real DOM.
export default defineConfig({
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
    // e2e/ is Playwright's; running it under vitest would hang on its fixtures.
    exclude: ["e2e/**", "node_modules/**", "dist/**"],
  },
});
