# feedback-as-data-audit

## Summary

This oracle guards the autopilot runner, which is where public feedback text reaches an
agent that writes code (FR-FBR-25b/c, Contract C27). It proves two things from the code.
First, feedback-derived text enters the agent prompt only inside the untrusted-data
envelope. Second, everything the runner sends out passes the egress sanitizer. Come here
after touching `crates/feedbackmonk-runner/src/{prompt,report,poll,client}.rs` or
`analyst/`.

## Probes

Comments and `#[cfg(test)]` modules are stripped before scanning.

- **A: envelope.**
  - `prompt.rs` defines `wrap_untrusted`, and no other runner file names the envelope
    delimiters.
  - In `assemble`, the order's `title`/`instructions` appear only in the owner-authored
    `None =>` arm or as arguments of `render_untrusted_block`, and recommendation fields
    (`rec.`) only inside it (DEC-FBR-IMPL-33: a recommendation-grounded order's text is
    copied model output, so it is data).
  - `assemble` routes the recommendation through
    `wrap_untrusted(render_untrusted_block(..))`, and nothing else calls
    `render_untrusted_block`.
- **B: egress, checked per function.**
  - Every `.runner_transition(..)` `result_ref` and `failure_reason`, and every
    `.post_recommendation(..)` payload, must be `None` or a variable bound in the same
    function from `sanitize_outbound`, `sanitize_clean` or `failure_reason_for_egress`.
  - The last two must themselves call `sanitize_outbound`.
  - No file but `client.rs` may name an HTTP client (`reqwest`, `hyper`, `ureq`).
- **C: corpus (`--full`).** Runs `cargo test -p feedbackmonk-api --test feedback_injection_corpus`.

## What it does not see

Where the API gets an order's text. It relies on the runner-side rule that a
recommendation-grounded order's title and instructions are data. An owner-authored order
(`recommendation: None`) is trusted by construction, since C31 lets only the owner write it.

## Invocation

```bash
python .claude/project-oracles/feedback-as-data-audit/oracle.py [--full]
```

It exits 0 on pass, 1 on fail, and 2 on an environment error.

## Adversarial self-test (2026-09-30)

Each mutation ran against a scratch copy of `crates/feedbackmonk-runner`, and each exited 1:

1. `report.rs` restored to its pre-2026-09-30 form, where `report_failed` sent a reason
   that was only PII-scrubbed. Caught as "sends `Some(&scrubbed)` without the egress
   chokepoint".
2. `assemble` pushing `rec.title` into `instructions`. Caught as "reads the recommendation
   in the trusted instruction layer".
3. `poll.rs` gaining a `reqwest::Client::new().post(..)`. Caught as "opens its own HTTP
   path".

The previous version checked egress at the level of whole files, so it passed mutation 1.
That was a real leak, fixed the same day.

Added with DEC-FBR-IMPL-33 (2026-09-30), also against scratch copies, each exiting 1:

4. `prompt.rs` restored to its pre-DEC-FBR-IMPL-33 form, which put a derived order's copied
   title and instructions in the trusted layer. Caught as "puts the order's title/instructions
   in the trusted layer" and "no owner-authored `None =>` arm".
5. `instructions.push_str(&rec.body)` beside `DERIVED_TASK`. Caught as "reads the
   recommendation in the trusted instruction layer".

**What Probe A is, honestly (second critic pass, 2026-09-30):** a tripwire for the plain shapes of
the regression, not the enforcement. It is pattern matching over `assemble`, and six more
elaborate rewrites pass it (a `Some(rec,)` or `rec @ _` pattern, destructuring
`ClaimedOrder { title: t, .. }`, `(*order).instructions`, `let o = order`, a helper function
outside `assemble`, a block expression inside `render_untrusted_block`'s arguments). All six
fail the behavioural tests, which are what enforce the trust split:
`assemble_keeps_feedback_out_of_the_trusted_layer`,
`injection_surviving_into_the_recommendation_title_stays_data` (prompt.rs) and corpus case (g).
Making the probe a proof would need a real Rust parser; the tests make that unnecessary.

After an independent critic showed two bypasses of that version (2026-09-30), Probe A requires
exactly one `match &order.recommendation` and no other mention of `recommendation`, allows the
Some-arm binding (whatever its name) only inside `render_untrusted_block`, and blanks string
literals before counting brackets. Each exits 1:

6. `Some(r) => { instructions.push_str(&r.body); ..` (renamed binding; passed before).
7. `if let Some(x) = &order.recommendation { instructions.push_str(&x.body); }` above the
   overrides block (passed before).
8. `instructions.push_str(&rec.rationale.clone().unwrap_or_default())` beside `DERIVED_TASK`.

And one that must stay clean: a string literal `"fixed (text) with } braces {"` in the trusted
layer exits 0.
