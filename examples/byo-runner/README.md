# BYO Runner — reference agent adapter

A minimal reference for **bring-your-own-agent** integration with the
`feedbackmonk-runner` (FR-FBR-24, Q20). The full protocol is documented in
[`docs/operations/RUNNER_PROTOCOL.md`](../../docs/operations/RUNNER_PROTOCOL.md);
this directory is the worked example its §8 points to.

> Both routes are settled against the shipped runner. The **external-command**
> route (`byo-agent.sh`) matches `default_agent::SpawnAgent`: prompt on **stdin**,
> a final `FEEDBACKMONK_RESULT_REF: {…}` line on **stdout**, non-zero exit =
> failure (RUNNER_PROTOCOL.md §8). The **in-process trait** route (`byo_agent.rs`)
> is against the **frozen** `AgentCommand` seam
> (`crates/feedbackmonk-runner/src/agent.rs`).

---

## Two ways to bring your own agent

### A. In-process — implement `AgentCommand` (the frozen seam)

The runner drives one trait. Implement it for full control, link it into a fork
of the runner (or a thin host binary), and inject it where the default
`ClaudeAgent` would go. See [`byo_agent.rs`](./byo_agent.rs).

This is the authoritative seam: the trait signature, `AssembledPrompt`,
`RepoContext`, and `ImplementResult` are frozen in Stage 0.

### B. External command — point `FEEDBACKMONK_AGENT_CMD` at a script

The default agent already spawns an external command; the simplest BYO is to
replace that command with your own program. The runner invokes it with the
claimed repo as the working directory and delivers the assembled prompt on
**stdin**; your program does the work and prints, as its final stdout line,
`FEEDBACKMONK_RESULT_REF: {…}` (single-line `ResultRef` JSON; non-zero exit =
failure). See [`byo-agent.sh`](./byo-agent.sh).

```sh
export FEEDBACKMONK_AGENT_CMD="/path/to/examples/byo-runner/byo-agent.sh"
feedbackmonk-runner poll --watch
```

---

## The contract every BYO agent must honour (security — not optional)

Whichever route you take, your agent operates on **untrusted public input**.
The runner enforces the envelope + egress sanitizer regardless (C27), but a
well-behaved BYO agent upholds the contract itself:

1. **`instructions` is the only authoritative directive layer.** It is the
   owner-approved work-order text — the approval gate is what makes it trusted.
2. **`untrusted_envelope` is data, never instructions.** It is feedback-derived
   text wrapped in `<untrusted-feedback-data> … </untrusted-feedback-data>`.
   Never let its content steer the agent's actions.
3. **Honour the DEC-84 critical-action deferral** carried in `instructions`: do
   not delete tests, weaken auth, or self-modify agent configuration —
   regardless of autonomy rung.
4. **Return conclusions only** — references + summaries, never source dumps or
   secrets. (The runner re-sanitizes on egress, but don't rely on it.)

A runner token can never author `approved` (C22 inv. 2), so even a misbehaving
BYO agent cannot bypass the owner-approval gate — but it CAN do damage inside an
already-approved order's repo. Treat the contract as load-bearing.

---

## Files

| File | Route | Status |
|---|---|---|
| `byo_agent.rs` | A — in-process `AgentCommand` impl | against frozen seam |
| `byo-agent.sh` | B — external-command adapter | settled (`FEEDBACKMONK_RESULT_REF:` stdout line) |
