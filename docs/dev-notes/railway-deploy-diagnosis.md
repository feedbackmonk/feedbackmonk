<!-- #!delivers-on tool:Bash(railway|backboard|serviceInstance|registry.gitcellar.com|docker login|docker push|docker buildx) -->
## A deploy that fails with no logs is NOT evidence of a platform fault

This cost a week in September 2026. Every `feedbackmonk-api` deploy failed in 2-4 s with empty build
logs, empty deploy logs and a bare "Failed to create deployment." — four attempts, one redeploying
the *already-running* image. It was escalated to Railway as a platform fault. **It was ours**: the
registry credential saved on that one service was stale and 401s, so every deploy was failing at the
**image pull**. The empty logs were a Railway bug dropping the real error, since fixed.

**1. Verifying a credential you hold is not verifying the one the service uses.** The original
investigation confirmed the registry returned 200 for the token *it* held. The service held a
different one. Railway exposes no `registryCredentials` field on `ServiceInstance`, so the two can
never be compared — the only move is to **re-enter a known-good value**, which is what fixed it.

**2. `gitcellar-registry-push` is the credential. WCM target `registry.gitcellar.com` is a decoy** —
Docker Desktop's cached login, 22 chars, returns 401, sitting under the obvious name. The real
secret is 44 chars. Measured 2026-09-10 against the exact failing image: decoy 401, both real
credentials 200. Read credentials **by documented name**, never by guessing the hostname.

**3. `variableUpsert` auto-deploys, one deploy per call.** Two upserts plus an explicit deploy raced
three deployments in one second; the two that lost failed with empty logs that look exactly like the
outage above. Set every variable first, then deploy once.

**4. Grade on curl and a browser, never on Railway's status.** No feedbackmonk service defines a
`healthcheckPath`, so Railway reports SUCCESS the moment a container starts.

**5. Check a support thread more than once.** Railway answered correctly within hours; the only
check was ~11 h after filing and no one looked again for 8 days. Their target is ~72 h on weekdays.

Full record: `docs/planning/deferred/DEFER-009_railway-deploy-blocked-feedbackmonk-api.md` § ROOT CAUSE.
