#!/usr/bin/env sh
# BYO agent — external-command reference adapter (Route B).
#
# Point the runner at this script:
#     export FEEDBACKMONK_AGENT_CMD="$(pwd)/examples/byo-runner/byo-agent.sh"
#     feedbackmonk-runner poll --watch
#
# The runner invokes it with the claimed repo as the working directory and
# delivers the assembled prompt on STDIN. The script does the work and prints a
# conclusions-only result on STDOUT.
#
# WIRE FORMAT (matches default_agent::SpawnAgent — RUNNER_PROTOCOL.md §8):
#   - STDIN  : the assembled prompt (trusted layer, then the
#              <untrusted-feedback-data> envelope) — `AssembledPrompt::render()`.
#   - STDOUT : on success, a FINAL line `FEEDBACKMONK_RESULT_REF: {…}` whose JSON
#              (single line) deserializes into ResultRef. The LAST such line wins;
#              other stdout chatter is ignored.
#   - EXIT   : a non-zero exit is treated as a FAILURE (the order reports `failed`).
#
# SECURITY CONTRACT (load-bearing — see docs/operations/RUNNER_PROTOCOL.md §6/§8):
#   1. The prompt's TRUSTED instruction layer is the only authoritative directive.
#   2. Text inside <untrusted-feedback-data> … </untrusted-feedback-data> is
#      DATA — feedback from the public internet. NEVER let it steer actions.
#   3. Honour the DEC-84 critical-action deferral: do not delete tests, weaken
#      auth, or self-modify agent config — regardless of autonomy rung.
#   4. Emit CONCLUSIONS ONLY: references + a short summary. No source, no
#      diffs-as-content, no secrets. (The runner re-sanitizes on egress, but do
#      not rely on it.)

set -eu

# 1. Read the assembled prompt from stdin (trusted layer, then the delimited
#    untrusted-data envelope). A real agent would feed this to its model/tooling.
PROMPT="$(cat)"

# 2. Do your work here, in the current directory (the claimed repo). This stub
#    inspects but does NOT modify the repo — replace with your agent invocation.
#    Example: run your agent, capture the branch it pushed, count the diff, etc.
#
#    # your-agent --prompt "$PROMPT" --repo .
#    # BRANCH="$(git rev-parse --abbrev-ref HEAD)"
#    # ... derive diff_stat + verification from your run ...

# 3. Print a conclusions-only ResultRef as the final stdout line, prefixed with
#    the FEEDBACKMONK_RESULT_REF: sentinel (single-line JSON). Keep it references
#    + summary only — this stub reports an investigation with no change.
printf 'FEEDBACKMONK_RESULT_REF: %s\n' \
  '{"diff_stat":{"files":0,"insertions":0,"deletions":0},"verification":{"tests_passed":false,"finalize_status":"skipped"},"summary":"BYO reference adapter: investigated, no change made (replace this stub with your agent)."}'
