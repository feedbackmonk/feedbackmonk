//! `feedbackmonk-runner` CLI entrypoint (Contract C26, FR-FBR-24 — P5b).
//!
//! Poll-based (cron/systemd/CI-portable; `--watch` for a long-running loop), NOT
//! webhook-driven — so it works for the owner's *local* repos with no public
//! endpoint. Subcommands (Stage-0 scaffold; Worker B/C finalize):
//!
//!   feedbackmonk-runner poll [--watch] [--sweep]   drive dispatched orders;
//!                                                   --sweep also runs the analyst
//!   feedbackmonk-runner mint-token --key <path>    customer-side token mint helper
//!
//! Stage 0 wires the argument shape + dispatch; the loop bodies live behind the
//! frozen `WorkOrderClient` / `implementer` / `analyst` seams (Worker B/C).

use std::process::ExitCode;

fn usage() {
    eprintln!(
        "feedbackmonk-runner — autonomous implementer + analyst host (FR-FBR-23/24)\n\
         \n\
         USAGE:\n\
         \x20 feedbackmonk-runner poll [--watch] [--sweep]\n\
         \x20 feedbackmonk-runner mint-token --key <path>\n\
         \n\
         Auth: set FEEDBACKMONK_RUNNER_TOKEN (a customer-minted runner write-token)\n\
         and FEEDBACKMONK_API_URL + FEEDBACKMONK_PROJECT_ID."
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        usage();
        return ExitCode::from(2);
    };

    match cmd {
        "poll" => {
            // SEAM (Worker B, Stage 1): build WorkOrderClient from env, loop
            // poll→claim→implement→report; with --sweep, run the analyst
            // (Worker C). --watch keeps the loop alive between ticks.
            eprintln!("feedbackmonk-runner poll: the runner loop is wired by Worker B (Stage 1).");
            ExitCode::from(1)
        }
        "mint-token" => {
            // SEAM (Worker B, Stage 1): mint a short-TTL EdDSA runner write-token
            // (scope=runner:write, jti) signed by the customer's runner-class
            // private key. feedbackmonk never holds the private key (DEC-FBR-04).
            eprintln!("feedbackmonk-runner mint-token: the mint helper is wired by Worker B (Stage 1).");
            ExitCode::from(1)
        }
        "-h" | "--help" | "help" => {
            usage();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown subcommand: {other}\n");
            usage();
            ExitCode::from(2)
        }
    }
}
