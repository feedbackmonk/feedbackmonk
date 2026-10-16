// uldf-template: testing-liveness v1
// Vendored from <home>/templates/testing/playwright/ (testing-runtime TLV-7..12, DEC-745). Change
// the template and re-copy; the `test-liveness` oracle reports a copy older than the template.
//
// The Playwright end of the machine-wide browser-test queue. `enterQueue` waits, with no limit,
// for one of the machine's browser-test slots (`<home>/scripts/test_queue.py acquire`), holding it
// by this runner's pid. It then adopts the project's test server, or starts it detached so that no
// run owns it. `leaveQueue` frees the slot. A runner that dies frees its slot too: the next waiter
// finds the pid dead. Wire both through `queue-setup.ts` and `queue-teardown.ts`, or call them
// from a globalSetup the project already has.
//
// The server is declared in the config's `metadata`, never in `webServer`, because the run that
// starts a `webServer` kills it at exit, under a peer's run that was using it (TLV-11):
//
//     metadata: { testServer: { url: BASE_URL, command: ["pnpm", "run", "dev"] } },

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import type { FullConfig } from "@playwright/test";

export interface TestServer {
  /** What answers when the server is up: any HTTP response counts. */
  url: string;
  /** The server's own start command, run detached in the project directory. */
  command: string[];
  /** Where its output goes; default `<project>/.test-server/server.log`. */
  log?: string;
}

function queueScript(): string | null {
  const home = process.env.ULDF_HOME ?? join(homedir(), ".claude");
  const script = join(home, "scripts", "test_queue.py");
  return existsSync(script) ? script : null;
}

function python(): string {
  return process.env.ULDF_PYTHON ?? (process.platform === "win32" ? "python" : "python3");
}

function projectDir(config: FullConfig): string {
  return config.configFile ? dirname(config.configFile) : config.rootDir;
}

function run(args: string[], what: string): void {
  // No timeout: a queue wait and a server start wait while their holders are alive, and fail at
  // once when one dies (TLV-7, 12).
  const result = spawnSync(python(), args, { stdio: ["ignore", "inherit", "inherit"] });
  if (result.error) throw new Error(`${what}: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`${what} failed (exit ${result.status}); its output is above`);
}

export function enterQueue(config: FullConfig): void {
  const script = queueScript();
  if (!script) {
    console.log("test-queue: <home>/scripts/test_queue.py is absent on this machine; running unqueued (TLV-10)");
    return;
  }
  const project = projectDir(config);
  run([script, "acquire", "--pid", String(process.pid), "--project", project], "test-queue acquire");
  const server = (config.metadata as { testServer?: TestServer } | undefined)?.testServer;
  if (server) {
    const args = [script, "server", "--url", server.url, "--project", project];
    if (server.log) args.push("--log", server.log);
    run([...args, "--", ...server.command], "test-queue server");
  }
}

export function leaveQueue(): void {
  const script = queueScript();
  if (script) run([script, "release", "--pid", String(process.pid)], "test-queue release");
}
