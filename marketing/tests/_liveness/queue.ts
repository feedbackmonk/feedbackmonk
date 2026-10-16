// uldf-template: testing-liveness v3
// Vendored from <home>/templates/testing/playwright/ (testing-runtime TLV-7..12, DEC-745). Change
// the template and re-copy; the `test-liveness` oracle reports a copy older than the template.
//
// The Playwright end of the machine-wide browser-test queue. `enterQueue` waits, with no limit,
// for one of the machine's browser-test slots (`<home>/scripts/test_queue.py acquire`), holding it
// by this runner's pid. It then adopts each of the project's test servers, or starts it detached so
// that no run owns it. `leaveQueue` frees the slot. A runner that dies frees its slot too: the next
// waiter finds the pid dead. Wire both through `queue-setup.ts` and `queue-teardown.ts`, or `await
// enterQueue(config)` first thing in a globalSetup the project already has.
//
// The servers are declared in the config's `metadata`, never in `webServer`, because the run that
// starts a `webServer` kills it at exit, under a peer's run that was using it (TLV-11). One server,
// or an array of them, adopted or started in order:
//
//     metadata: { testServer: { url: BASE_URL, command: ["pnpm", "run", "dev"] } },
//
// On a machine without the framework (a contributor, a CI runner), there is no queue, and the
// servers are still adopted or started the same way, so an adopted lane runs anywhere.

import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, openSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, isAbsolute, join, resolve } from "node:path";
import type { FullConfig } from "@playwright/test";

export interface TestServer {
  /** What answers when the server is up: any HTTP response counts. */
  url: string;
  /** The server's own start command, run detached in `cwd`. It must not build: a server outlives
   *  the run, so a later run would adopt it serving the first run's build. Build in globalSetup. */
  command: string[];
  /** Where the command runs; default the nearest directory above the config holding a
   *  `package.json`. A relative path is taken from there. */
  cwd?: string;
  /** Variables only the server needs, added to the runner's environment for its start. */
  env?: Record<string, string>;
  /** Where its output goes; default `<cwd>/.test-server/server-<port>.log`, one per server. */
  log?: string;
  /** Proves the server answering is serving THIS checkout: a nonce is written to `file` (under
   *  `cwd`) and must come back from `url` + `path`. An adopted server started by another checkout
   *  serves that checkout's code, and fails here naming what it answered. */
  stamp?: { file: string; path: string };
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
  const start = config.configFile ? dirname(config.configFile) : config.rootDir;
  for (let dir = start; ; dir = dirname(dir)) {
    if (existsSync(join(dir, "package.json"))) return dir;
    if (dirname(dir) === dir) return start;
  }
}

function servers(config: FullConfig): TestServer[] {
  const declared = (config.metadata as { testServer?: TestServer | TestServer[] } | undefined)?.testServer;
  return declared === undefined ? [] : Array.isArray(declared) ? declared : [declared];
}

function serverDir(project: string, server: TestServer): string {
  return server.cwd === undefined ? project : resolve(project, server.cwd);
}

function serverLog(cwd: string, server: TestServer): string {
  if (server.log !== undefined) return isAbsolute(server.log) ? server.log : resolve(cwd, server.log);
  const port = new URL(server.url).port || "default";
  return join(cwd, ".test-server", `server-${port}.log`);
}

function run(args: string[], what: string, env: NodeJS.ProcessEnv = process.env): void {
  // No timeout: a queue wait and a server start wait while their holders are alive, and fail at
  // once when one dies (TLV-7, 12).
  const result = spawnSync(python(), args, { stdio: ["ignore", "inherit", "inherit"], env });
  if (result.error) throw new Error(`${what}: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`${what} failed (exit ${result.status}); its output is above`);
}

async function answers(url: string): Promise<boolean> {
  try {
    await fetch(url);
    return true;
  } catch {
    return false;
  }
}

function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function tail(log: string): string {
  try {
    return readFileSync(log, "utf-8").split(/\r?\n/).slice(-20).join("\n");
  } catch {
    return "";
  }
}

/** Without the queue: adopt the URL, or start the command detached and wait for it to answer for
 *  as long as it lives, as `test_queue.py server` does (TLV-12). */
async function startUnqueued(server: TestServer, cwd: string, log: string): Promise<void> {
  if (await answers(server.url)) {
    console.log(`test-queue: adopted the server answering at ${server.url}`);
    return;
  }
  mkdirSync(dirname(log), { recursive: true });
  const out = openSync(log, "a");
  const [command, ...args] = server.command;
  if (command === undefined) throw new Error(`the test server for ${server.url} has an empty command`);
  // Windows resolves `pnpm` to `pnpm.cmd` only through a shell, which takes one quoted line.
  const quote = (arg: string) => (/[\s"]/.test(arg) ? `"${arg.replace(/"/g, '\\"')}"` : arg);
  const options = { cwd, detached: true, stdio: ["ignore", out, out] as ["ignore", number, number],
                    env: { ...process.env, ...server.env }, windowsHide: true };
  const child = process.platform === "win32"
    ? spawn([command, ...args].map(quote).join(" "), { ...options, shell: true })
    : spawn(command, args, options);
  child.unref();
  const pid = child.pid;
  if (pid === undefined) throw new Error(`could not start the test server for ${server.url}: ${command}`);
  console.log(`test-queue: started the test server for ${server.url} (pid ${pid}, unqueued); log ${log}`);
  while (!(await answers(server.url))) {
    if (!alive(pid)) {
      throw new Error(`the test server for ${server.url} exited before it answered (pid ${pid}); `
        + `its log is ${log}:\n${tail(log)}`);
    }
    await new Promise((r) => setTimeout(r, 250));
  }
}

async function proveStamp(server: TestServer, cwd: string): Promise<void> {
  if (!server.stamp) return;
  const nonce = `uldf-${process.pid}-${Date.now()}-${Math.random().toString(36).slice(2)}`;
  const file = resolve(cwd, server.stamp.file);
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, nonce, "utf-8");
  const url = new URL(server.stamp.path, server.url).href;
  let got: string;
  try {
    got = (await (await fetch(url)).text()).trim();
  } catch (error) {
    got = `(no answer: ${error instanceof Error ? error.message : String(error)})`;
  }
  if (got !== nonce) {
    throw new Error(`the server at ${server.url} is not serving this checkout (${cwd}): ${url} `
      + `answered ${JSON.stringify(got.slice(0, 200))}, not the stamp just written to ${file}. `
      + `Another checkout's server, or another app, holds the port; give this checkout a port of its own.`);
  }
}

export async function enterQueue(config: FullConfig): Promise<void> {
  const script = queueScript();
  const project = projectDir(config);
  const declared = servers(config);
  if (!script) {
    console.log("test-queue: <home>/scripts/test_queue.py is absent on this machine; running unqueued (TLV-10)");
  } else {
    run([script, "acquire", "--pid", String(process.pid), "--project", project], "test-queue acquire");
  }
  for (const server of declared) {
    const cwd = serverDir(project, server);
    const log = serverLog(cwd, server);
    if (script) {
      run([script, "server", "--url", server.url, "--project", cwd, "--log", log, "--", ...server.command],
          "test-queue server", { ...process.env, ...server.env });
    } else {
      await startUnqueued(server, cwd, log);
    }
    await proveStamp(server, cwd);
  }
}

export function leaveQueue(): void {
  const script = queueScript();
  if (script) run([script, "release", "--pid", String(process.pid)], "test-queue release");
}
