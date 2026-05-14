// One-shot font fetch for the marketing site.
//
// Per BRAND.md §Typography: Inter + JetBrains Mono are self-hosted (no
// Google Fonts CDN — BRAND.md mandate extending DEC-FBR-02). The `.woff2`
// files are NOT committed (binary, ~700 KB combined); operators run this
// script once after `npm install`.
//
// Sources:
//   Inter Variable — rsms.me (SIL OFL 1.1, safe to redistribute)
//   JetBrains Mono Variable — JetBrains/JetBrainsMono (Apache-2.0)
//
// Plain Node ≥18, no extra deps. Idempotent — re-running is safe.

import { mkdirSync, statSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const root = dirname(dirname(__filename)); // marketing/
const fontsDir = join(root, 'public', 'fonts');
mkdirSync(fontsDir, { recursive: true });

const FONTS = [
  {
    name: 'InterVariable.woff2',
    url: 'https://rsms.me/inter/font-files/InterVariable.woff2',
    license: 'SIL OFL 1.1 — rsms.me/inter',
  },
  {
    name: 'JetBrainsMono-Variable.woff2',
    url: 'https://raw.githubusercontent.com/JetBrains/JetBrainsMono/master/fonts/webfonts/JetBrainsMono%5Bwght%5D.woff2',
    license: 'Apache-2.0 — JetBrains/JetBrainsMono',
  },
];

async function fetchOne({ name, url, license }) {
  const outPath = join(fontsDir, name);
  if (existsSync(outPath)) {
    const size = statSync(outPath).size;
    console.log(`  skip ${name} (already present, ${size} bytes)`);
    return;
  }
  const res = await fetch(url);
  if (!res.ok) {
    throw new Error(`failed to fetch ${url}: HTTP ${res.status}`);
  }
  const bytes = new Uint8Array(await res.arrayBuffer());
  writeFileSync(outPath, bytes);
  console.log(`  fetched ${name} (${bytes.byteLength} bytes — ${license})`);
}

console.log('fetching self-hosted fonts into public/fonts/ ...');
for (const f of FONTS) {
  await fetchOne(f);
}
console.log('done.');
