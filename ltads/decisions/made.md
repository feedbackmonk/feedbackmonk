# Decisions Made

See `docs/specs/DECISIONS.md` for DEC-FBR-01..10 (all RESOLVED).

P0 plan tech defaults (do not re-litigate without escalating via `/0-uldf-ltads-admin decision`):
- Web framework: `axum`
- Query layer: `sqlx` with compile-time checking + `sqlx::migrate!`; offline cache via `cargo sqlx prepare`
- Password hash: `argon2`
- JWT crypto (Stage 2): `jsonwebtoken` v9 (EdDSA), `ed25519-dalek` fallback
- Env-var prefix: `FEEDBACKR_`
- Backend dev port: `14304` (to be registered in MACHINE_CONFIG.md at Stage 1 start)
- Repository enforcement: three-leg — type system + oracle + clippy/cargo-deny
