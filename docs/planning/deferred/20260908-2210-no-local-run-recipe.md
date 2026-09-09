---
status: open
source: SessionHelm, session d4161639 (wiring a launch button for FeedbackMonk in Session Helm)
origin: needed "how do I run this app locally"; LOCAL_DEV.md documents building and testing but never running
harm: witnessed
beneficiary: product
---

# `docs/operations/LOCAL_DEV.md` tells you how to build and test, never how to run

Measured 2026-09-08, trying to compose a one-button local launch. `LOCAL_DEV.md` has sections for
the Postgres container, Building (online + offline), the CI-parity gate, Running **tests**, and the
Verification Oracle — and no section for starting the app. The two things a first run actually
trips over are documented nowhere:

1. **`FEEDBACKMONK_SESSION_SECRET` is mandatory and undocumented.**
   `crates/feedbackmonk-api/src/main.rs` (~line 559) hard-fails with
   `"FEEDBACKMONK_SESSION_SECRET not set (expected 64 hex chars)"`. It appears in `LOCAL_DEV.md`
   zero times, and the repo's `.env` holds only `DATABASE_URL`.
2. **The API does not read `.env` at all.** `main.rs`'s own header says so — env is injected by the
   parent process, there is no `dotenv` dependency. So the `.env` file that exists is used by cargo
   tooling, not by the running server, and a reader who assumes otherwise gets failure (1) while
   staring at a file that looks like it should have prevented it.

Add to that: the app is **two processes plus a container** (`docker start feedbackmonk-pg-dev` →
`cargo run -p feedbackmonk-api` on 14304 → `npm run dev` in `admin-ui/` on 14204, which proxies
`/api` to the former), and the admin UI comes up looking broken rather than pending when the API is
absent.

**What to try** — a `## Running the app locally` section in `LOCAL_DEV.md`, and ideally the script
it describes (`scripts/` currently holds only CI-parity, e2e and oracle helpers; there is no
`start-*` or `*detached*` anywhere). It needs to state the secret requirement, that env is
parent-injected, the container-then-API-then-UI order, and that the first `cargo run` compiles the
workspace so the UI will briefly serve against a dead API.

**Interim recipe now in use** (the Session Helm project-action button for this project, unpressed as
of filing — treat as a starting point, not as verified):

```
docker start feedbackmonk-pg-dev
# env injected by the caller:
#   DATABASE_URL=postgres://postgres:dev@localhost:5433/feedbackmonk_dev
#   FEEDBACKMONK_SESSION_SECRET=<64 hex chars>
cargo run -p feedbackmonk-api          # own window
cd admin-ui && npm run dev             # http://localhost:14204
```

The secret there was generated ad hoc for local dev. **Decide where a dev secret should come
from** — a documented `openssl rand -hex 32` line the reader runs, a gitignored `.env.local` the
launcher sources, or a dev-only default the binary accepts with a loud warning. Right now every
developer invents their own and none of them writes it down, which is how this brief happened.

**Ruled out**: pointing a local button at `feedback.gitcellar.com`. CLAUDE.md is explicit that it is
production and Railway-blocked — "Change no env var, setting or image pin there."
