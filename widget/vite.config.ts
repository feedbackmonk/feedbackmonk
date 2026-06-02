import { defineConfig } from "vite";
import { resolve } from "node:path";

// feedbackmonk widget bundler config.
//
// Load-bearing constraints (enforced by .claude/oracles/widget-bundle-size/):
//   - Bundle total `widget/dist/*.{js,mjs,css}` <= 30720 bytes (FR-FBR-04)
//   - No third-party tracker hostnames anywhere in built output (DEC-FBR-02)
//
// CSP discipline (no `unsafe-inline`/`unsafe-eval` required by embedders):
//   - terser `compress.unsafe: false` (no `eval` rewrites)
//   - terser `mangle.eval: false`
//   - no `iife` wrapper that injects code via Function() — emit ESM `widget.js`
//     that customers load with `<script type="module" src="…/widget.js">`.
//   - styles emitted as a sibling `widget.css` file (loaded with <link>) so
//     embedders' style-src CSP does not need `unsafe-inline`.

export default defineConfig({
  build: {
    target: "es2020",
    outDir: "dist",
    emptyOutDir: true,
    cssCodeSplit: false,
    cssMinify: true,
    sourcemap: false,
    reportCompressedSize: true,
    lib: {
      entry: resolve(__dirname, "src/widget.ts"),
      name: "FeedbackMonk",
      formats: ["es"],
      fileName: () => "widget.js",
    },
    rollupOptions: {
      output: {
        // Stable, unhashed names: customers load `widget.js`; the redaction
        // editor is the same-origin `redact.js` chunk fetched lazily on first
        // use. No content hashes — `dist/` is committed for the size oracle, so
        // churn-free filenames keep the diff reviewable.
        entryFileNames: "widget.js",
        chunkFileNames: "[name].js",
        assetFileNames: (asset) => {
          if (asset.name === "style.css" || asset.name === "widget.css") {
            return "widget.css";
          }
          return "[name][extname]";
        },
      },
    },
    minify: "terser",
    terserOptions: {
      compress: {
        passes: 2,
        unsafe: false,
        drop_console: true,
        drop_debugger: true,
      },
      mangle: {
        eval: false,
      },
      format: {
        comments: false,
      },
    },
  },
});
