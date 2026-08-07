# pid-orphan-detector test-fixtures

The validate.{sh,ps1} scripts build their fixtures dynamically (alive PIDs are platform-specific and must be sourced from the live process table — pre-baked PID values would invalidate every cross-machine run). This directory exists for parity with sibling oracles (`archive-retention/test-fixtures/`, `dispatchable-sessions/test-fixtures/`) and for any future static fixture additions (e.g., known-malformed `.pid` content samples that don't depend on liveness).

Currently empty — see `validate.sh` and `validate.ps1` for the in-line fixture build.
