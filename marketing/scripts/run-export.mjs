// DEC-FBR-IMPL-05 — Cross-platform dispatcher for the Rust→JSON pricing SSOT
// export. Invoked by Astro's `prebuild` npm script. Picks `.ps1` on Windows,
// `.sh` elsewhere, and runs the cargo example binary with stdout redirected
// into `marketing/src/data/tier_quotas.json` (gitignored).
//
// Plain Node ≥18, no extra deps.

import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';
import { mkdirSync, statSync, writeFileSync } from 'node:fs';

const __filename = fileURLToPath(import.meta.url);
const scriptsDir = dirname(__filename);
const repoRoot = resolve(scriptsDir, '..', '..');
const outPath = join(repoRoot, 'marketing', 'src', 'data', 'tier_quotas.json');

mkdirSync(dirname(outPath), { recursive: true });

// Run cargo directly from Node so we don't need a child shell to interpret
// the `.sh` / `.ps1` shims. The shims remain available for manual invocation
// (and as documentation), but the prebuild hot path is this single
// `cargo run` — same binary, same output, simpler error surface.
const result = spawnSync(
  'cargo',
  ['run', '--quiet', '-p', 'feedbackmonk-core', '--example', 'export_tier_quotas'],
  { cwd: repoRoot, encoding: 'utf8', shell: process.platform === 'win32' },
);

if (result.error) {
  console.error('failed to spawn cargo:', result.error.message);
  process.exit(1);
}
if (result.status !== 0) {
  console.error(result.stderr || `cargo exited with status ${result.status}`);
  process.exit(result.status ?? 1);
}

writeFileSync(outPath, result.stdout);
const size = statSync(outPath).size;
console.log(`wrote marketing/src/data/tier_quotas.json (${size} bytes)`);
