# Deferred Decisions

Per P0 plan §Deferred Decisions:
- Redis-backed rate-limiter → v1.1 (default: in-memory `governor`)
- Prometheus metrics → v1.1 (default: tracing-emitted counters in logs)
- Email provider for signup verify → Stage 2 Worker A planning (default: Mailpit dev / SMTP env-var prod)
- Admin session mechanism → Stage 2 Worker A (default: signed cookie + HMAC env var)
