# Provision `feedbackmonk.com` + migrate GitCellar onto it (PF-SAAS-STANDUP-01)

**Status (2026-08-30): BLOCKED ON THE OWNER — ops, not code.** FR-FBR-32/33 (the DEC-FBR-14 commercial
hosting shape) are complete and tested; what is missing is somewhere to run them. Full runbook:
`docs/operations/SAAS_HOSTING.md`.

## 1. Provision the deployment

Hosting account, `*.feedbackmonk.com` + `app.feedbackmonk.com` DNS, wildcard TLS (DNS-01 needs a Caddy
build with the provider module + an API token), and `FEEDBACKMONK_ROOT_DOMAIN` /
`FEEDBACKMONK_ADMIN_HOST` / `FEEDBACKMONK_TRUSTED_PROXY_HOPS=1`.

**Recommendation: run it separately from GitCellar's Railway.** The vendor's SaaS living inside
customer #1's infrastructure is precisely the arrangement DEC-FBR-14 exists to undo; reproducing it
would leave the dogfooding gap where it was.

## 2. Cut GitCellar over

`SAAS_HOSTING.md` § 4, ordered and reversible. The GitCellar side is filed in that repo as **DEFER-084**
(`feedbackmonk-saas-tenant-cutover`), which carries the DNS/data/decommission/doc work.

**Coordinate with the GitCellar side before touching DNS**: live GitCellar sessions exist on this
machine and may be measuring against `feedback.gitcellar.com`.

**No GitCellar source file is edited by any step** — `triage.gitcellar.com` keeps working as an
operator-registered admin alias that 301s to the canonical admin host (DEC-FBR-IMPL-27), so `TRIAGE_URL`
is never touched. A plan that requires editing it has misread DEC-FBR-14.

## Why it matters beyond itself

The cutover retires the whole `gitcellar-instance-redeploy.md` stack at once — a SaaS instance runs
current code with every migration applied. Weigh that against three or four separate Railway redeploys.
