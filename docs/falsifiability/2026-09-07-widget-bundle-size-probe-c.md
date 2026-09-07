# Falsifiability Receipt: widget-bundle-size-probe-c

**Date**: 2026-09-07
**Author**: `/0-uldf-finalize` Phase 11.6, run `fin-20260907-i18n-stage1` (i18n Stage 1 commit)
**Verdict**: FALSIFIABLE

## Claim

> "The 30 KB cap holds for the English page-load set; locale chunks carry a per-chunk cap."
> — `docs/specs/SPECIFICATION.md` FR-FBR-35, and the `widget-bundle-size` oracle row:
> "Probe C per-chunk cap 4,096 B".

**Reported at**: FR-FBR-35 (`docs/specs/SPECIFICATION.md`); `.claude/oracles/widget-bundle-size/manifest.json`
version 1.1.0; this commit's message ("widget bytes 29,836/30,720").

Why this claim and not another: Probe C is **new in this commit** and it is the only defense of the
30-chunk budget. Its three new i18n sibling oracles (`i18n-catalog-integrity`, `i18n-literal-ratchet`,
`translation-gap-status`) each ship an invertible `--self-test` that was run and passed at this same
finalize; `widget-bundle-size` declares `self_test_unix: null`, so `--self-test` falls through to an
ordinary run and demonstrates nothing. Under FALSIFY-02 that makes Probe C the one claim-bearing,
receipt-less verification surface in the diff.

## Assertion under test

`.claude/oracles/widget-bundle-size/oracle.py:147`

```python
offenders = [(f, sz) for f, sz in chunks if sz > LOCALE_CHUNK_CAP_BYTES]
```

with `LOCALE_CHUNK_CAP_BYTES = 4 * 1024` at `oracle.py:74`.

The specific smell being ruled out is **empty fixture / vacuous PASS**: at rest every chunk is
tiny (largest 53 B against a 4,096 B cap, 4,043 B of headroom), which is exactly the shape where an
assertion can be green because it is never actually exercised. The oracle's own output even
distinguishes a "vacuous PASS" branch (`oracle.py:227,244`), so the question is real.

## Neutering

Padded exactly one real chunk past the cap and left everything else untouched — no edit to the
oracle, the cap constant, the slicer, or any other chunk:

```bash
cp widget/dist/locales/bg.js /tmp/bg.js.bak
python -c "p='widget/dist/locales/bg.js'; s=open(p,encoding='utf-8').read(); \
  open(p,'w',encoding='utf-8',newline='').write(s + '\n// falsifiability probe padding\n' + 'x'*4200)"
# 53 B -> 4286 B
bash .claude/oracles/widget-bundle-size/oracle.sh
```

## Observed RED

```text
FAIL widget-bundle-size (1 probe(s) failed)
  tracker-list hash: 7823d6e6dfe712b4c9ed07a85562688740a4b911f11ed6d6e648c745082cf629 (18 hostnames)

Probe C failure (per-locale catalog chunk exceeds 4096B cap per Contract C42):
  widget/dist/locales/bg.js  4286B  over_by=190B (cap=4096B)
  Remediation: a locale chunk is a flat map of ~40 short strings and nothing else. Check widget/scripts/slice-locales.mjs for leaked code, a non-widget namespace or duplicated keys. Never silently raise LOCALE_CHUNK_CAP_BYTES.
```

Exit code `1`. The failure names the offending file, its measured size, and the overage — it is a
usable diagnostic, not a bare non-zero exit.

## Restore

`cp /tmp/bg.js.bak widget/dist/locales/bg.js`, then a sha256 invariant check on the restored file:

```text
before sha: 73e1f0a257b7c911f97fbc694a6a76f2ff1fcaa51fa82cb6ae684f7b239ab939  (53 B)
after  sha: 73e1f0a257b7c911f97fbc694a6a76f2ff1fcaa51fa82cb6ae684f7b239ab939
RESTORE VERIFIED (sha invariant holds)
```

Re-ran the oracle after restore: `PASS widget-bundle-size`, exit `0`. The tree carries no residue of
this probe — `widget/dist/locales/bg.js` is byte-identical to its pre-probe state, and the diff this
commit stages is unaffected.

## Notes

**What this receipt does and does not buy.** It establishes that Probe C's comparison is live and
that a cap violation surfaces as a red exit with an actionable message. It does **not** substitute
for a self-test: a receipt is a one-off demonstration performed by hand at one commit, whereas a
`--self-test` re-demonstrates the same property on every future run and would catch a *later* edit
that quietly defeats the probe. The remaining gap — `widget-bundle-size` declares
`self_test_unix: null` / `self_test_windows: null` while its three siblings do not — is recorded in
`docs/planning/observations-ledger.md` (2026-09-07) rather than filed as a brief: it is a coverage
gap in verification machinery with no witnessed wrong verdict, which is precisely what that ledger
is for. The natural moment to close it is the next edit to this oracle.

**Probe A was not separately receipted** and is deliberately out of this receipt's scope: unlike
Probe C it is not new in this commit (it predates Stage 1 and has been actively firing against a
real, near-cap measurement — 29,836 B against 30,720 B, 884 B of headroom, i.e. 97% of budget
consumed). An assertion that close to its threshold on real data is not at risk of the vacuous-PASS
smell that motivated this receipt.
