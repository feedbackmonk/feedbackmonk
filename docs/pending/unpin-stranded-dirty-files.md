# Unpin the `stranded-dirty-files` oracle (PF-UNPIN-01)

**Status: the trigger has FIRED (measured 2026-08-30). This is now just work to do.**

**Trigger**: the framework baseline no longer ships the developer's Windows account name — i.e. ULDF
`DEFER-221` is fixed *and* synced to this machine. One-command test, which assembles both spellings
from the live account rather than embedding either (**do not paste the literal back in** — DEFER-003
records that this repo's previous version of this very note re-published the identifier it was
documenting the removal of):

```bash
U=$(id -un); grep -ciE "$U|$(printf %s "$U" | tr a-z A-Z | cut -c1-6)~1" ~/.claude/oracles/stranded-dirty-files/validate.ps1
```

→ `0` means fired. It returned `0` on 2026-08-30.

**Why the pin exists**: commit `5bf9878` (2026-05-17) scrubbed that account name to `someuser` in
`validate.ps1` lines 38-39, ahead of this repo's first public push. The framework baseline still
carried the unscrubbed form, so a blanket `/0-uldf-migrate-oracles` refresh reverts it — **observed
twice**: `d71c35a` (2026-08-06) and again during the DEC-405 refresh (`de297c3`, 2026-08-21). The
oracle was refreshed to current baseline *first*, the scrub re-applied, then pinned via
`.claude/oracles/stranded-dirty-files/.local-customized` — so it carries the DEC-405 InvariantCulture
date fix and diverges from baseline by 2 comment lines only.

**Action**: delete `.claude/oracles/stranded-dirty-files/.local-customized`, then confirm a refresh no
longer reverts the scrub. It is a `.claude/` write, hence DEC-84-gated for a worker session (DEFER-003).

**The cost of leaving it**: the pin now blocks every future framework fix to this oracle from reaching
this project. That is the whole reason to act.

**Upstream**: `DEFER-221` in the ULDF repo (`docs/planning/deferred/`).

Delete this file and the CLAUDE.md line together, in the commit that removes the pin.
