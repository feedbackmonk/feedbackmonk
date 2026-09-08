<!-- #!delivers-on tool:Bash(cargo |sqlx prepare|ci-local|git push) -->
## CI parity — run before every push (MANDATORY)

CI (`.github/workflows/ci.yml`) compiles **all targets** in **offline** mode
(`SQLX_OFFLINE=true cargo {build,test,clippy} --workspace --all-targets`, with
`RUSTFLAGS: -D warnings`). A plain `cargo build --workspace` or a targeted
`cargo test --test X` does **not** compile every test target and does **not**
deny warnings — so it silently skips the two failure classes that turn CI red:

1. **`sqlx::query!` in `tests/` with no cached metadata.** `cargo sqlx prepare
   --workspace` (no `-- --all-targets`) misses test-target queries → CI fails
   offline with *"no cached data for this query"*. **Always prepare with
   `cargo sqlx prepare --workspace -- --all-targets`** and commit `.sqlx/`.
2. **clippy/rustc warnings-as-errors** anywhere in the workspace.

**Before any `git push` of Rust changes, run the one-command gate** (it runs the
oracle + offline `clippy --all-targets -D warnings`, which compiles every target
and catches BOTH classes above):

```bash
bash scripts/ci-local.sh           # fast, DB-free (oracle + compile/lint gate)
bash scripts/ci-local.sh --tests   # also the suite (needs DATABASE_URL + Postgres)
```

(PowerShell: `pwsh scripts/ci-local.ps1 [-Tests]`.) Green here ⇒ green CI, modulo
runner-speed flakes; it is the gate `/0-uldf-finalize`'s push step depends on.

Its first step is the verification-oracle suite, which lives under
`.claude/project-oracles/` — **not** `.claude/oracles/`, which is the ULDF starter
pack on a different contract. `scripts/run-verification-oracles.sh` is the single
source of truth for that suite; CI and `ci-local` both call it.
