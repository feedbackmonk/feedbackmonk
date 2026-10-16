// uldf-template: testing-liveness v3
// Vendored from <home>/templates/testing/playwright/ (testing-runtime TLV-1..6, DEC-745). Change
// the template and re-copy; the `test-liveness` oracle reports a copy older than the template.
//
// How a test waits on this machine: it settles, then checks once (TLV-1), and the stall detector
// below is the backstop for every wait (TLV-3, 4). A spec imports `test` and `expect` from HERE
// instead of `@playwright/test`, and that one change puts it under the detector (TLV-15).
//
// The detector counts samples, never seconds. Every `cadenceMs` it reads each page's snapshot
// (`quiescence.ts`). It declares a stall after `stallSamples` consecutive counted samples that all
// see nothing in flight (in any page, or on the Node side through `busy()`), the same mutation
// count, and the same documents. A sample counts only when the runner took it on time: its gap
// from the previous sample and its round trip into the page are each within twice the cadence. A
// starved sample, which happens when the machine is busy or the Node loop is blocked, neither adds
// to the run nor resets it. So pending work, however slow, keeps the test alive indefinitely, and a
// wait that nothing left can satisfy fails within `stallSamples` on-time samples.

import { test as base, expect, type BrowserContext, type Request, type TestInfo } from "@playwright/test";
import { installQuiescence, type QuietSnapshot } from "./quiescence";

export { expect };

export interface LivenessOptions {
  /** Consecutive quiet, on-time samples that make a stall. */
  stallSamples: number;
  /** The sampling cadence. A cadence, not a bound: nothing fails because time passed. */
  cadenceMs: number;
  /** A `setTimeout` at or under this delay counts as work in flight (TLV-2). */
  shortTimerMs: number;
  /** URL substrings of requests that are not work: a stream the app holds open for its whole
   *  life (an EventSource, a long poll) would otherwise keep every wait alive. */
  ignoreUrls: string[];
}

export const LIVENESS_DEFAULTS: LivenessOptions = {
  stallSamples: 40, cadenceMs: 250, shortTimerMs: 1000, ignoreUrls: [],
};

const nodeBusy = new Map<number, string>();
let nodeSeq = 0;

/** TLV-5: a wait outside the page (a file, a daemon, a child process) declares itself, or the
 *  detector may read a quiet page as a stall. */
export async function busy<T>(work: Promise<T> | (() => Promise<T>), label: string): Promise<T> {
  const id = ++nodeSeq;
  nodeBusy.set(id, label);
  try {
    return await (typeof work === "function" ? work() : work);
  } finally {
    nodeBusy.delete(id);
  }
}

/** TLV-1: wait until the page is settled. Then read the value once and assert it with a
 *  non-retrying matcher, so a wrong value fails at once and shows what it was:
 *
 *      await settled(page);
 *      expect(await row.getAttribute("aria-selected")).toBe("true");
 */
export async function settled(page: import("@playwright/test").Page): Promise<void> {
  const installed = await page.evaluate(() => typeof window.__uldfQuiet?.settled === "function");
  if (!installed) {
    throw new Error("settled(): this page has no quiescence counter -- import `test` from the "
      + "liveness fixture, or call watchContext() on a context you built yourself");
  }
  await page.evaluate(() => window.__uldfQuiet!.settled());
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export interface Watcher {
  /** Stop sampling; throws the stall report when the detector ended the test. */
  stop(): Promise<void>;
}

/** TLV-6: attach the counter and the detector to a context the test built itself (a CDP-attached
 *  web view, a persistent context). The fixture below calls this for the default context. */
export async function watchContext(context: BrowserContext, testInfo: TestInfo,
                                   options: Partial<LivenessOptions> = {}): Promise<Watcher> {
  const o = { ...LIVENESS_DEFAULTS, ...options };
  await context.addInitScript(installQuiescence, o.shortTimerMs);
  for (const page of context.pages()) {
    await page.evaluate(installQuiescence, o.shortTimerMs).catch(() => undefined);
  }

  // Every request the browser has in flight for this context, seen from the Node side: module
  // scripts, a dynamic `import()`, the document itself, images. The page-side counter cannot see
  // these, and a cold dev server transforming them is slow, real work (TLV-2).
  const requests = new Map<Request, string>();
  const onRequest = (r: Request) => {
    const url = r.url();
    if (!o.ignoreUrls.some((part) => url.includes(part))) requests.set(r, `${r.resourceType()} ${url}`);
  };
  const onDone = (r: Request) => { requests.delete(r); };
  context.on("request", onRequest);
  context.on("requestfinished", onDone);
  context.on("requestfailed", onDone);
  const openRequests = () => {
    for (const r of [...requests.keys()]) {
      let closed = false;
      try {
        closed = r.frame().page().isClosed();
      } catch {
        closed = false;   // a service worker's request has no frame; its own events end it
      }
      if (closed) requests.delete(r);
    }
    return [...requests.values()];
  };

  let running = true;
  let stall: string | null = null;
  const loop = (async () => {
    let quiet = 0;
    let counted = 0;
    let starved = 0;
    let lastKey = "";
    let armed = false;
    let last: Array<QuietSnapshot | null> = [];
    let previous = performance.now();
    while (running) {
      await sleep(o.cadenceMs);
      if (!running) return;
      const now = performance.now();
      const gap = now - previous;
      previous = now;
      const pages = context.pages();
      let snaps: Array<QuietSnapshot | null>;
      try {
        snaps = await Promise.all(pages.map((p) =>
          p.evaluate(() => window.__uldfQuiet?.snapshot() ?? null).catch(() => null)));
      } catch {
        quiet = 0;
        continue;
      }
      const roundTrip = performance.now() - now;
      if (gap > 2 * o.cadenceMs || roundTrip > 2 * o.cadenceMs) {
        starved++;
        continue;
      }
      counted++;
      armed ||= snaps.some((s) => s !== null);
      const key = JSON.stringify(snaps.map((s) => s && [s.doc, s.href, s.mutations]));
      const working = nodeBusy.size > 0 || openRequests().length > 0
        || snaps.some((s) => s === null || s.inflight.length > 0);
      if (!armed || working || key !== lastKey) quiet = 0;
      else quiet++;
      lastKey = key;
      last = snaps;
      if (quiet >= o.stallSamples) {
        stall = report(pages.map((p) => p.url()), last, counted, starved, o);
        stall += await excerpt(context);
        running = false;
        await context.close().catch(() => undefined);
        return;
      }
    }
  })();

  return {
    async stop() {
      running = false;
      await loop;
      context.off("request", onRequest);
      context.off("requestfinished", onDone);
      context.off("requestfailed", onDone);
      if (stall) {
        testInfo.annotations.push({ type: "stall", description: stall.slice(0, stall.indexOf("\n")) });
        throw new Error(stall);
      }
    },
  };
}

function report(urls: string[], snaps: Array<QuietSnapshot | null>, counted: number, starved: number,
                o: LivenessOptions): string {
  const lines = [
    `STALL (testing-runtime TLV-4): ${o.stallSamples} consecutive on-time samples, `
      + `${o.cadenceMs} ms apart, saw nothing in flight, no DOM change and no navigation, so the `
      + `wait in progress could never be satisfied. The test's context was closed to end that wait; `
      + `the error above it names what the test was waiting for.`,
    `samples: ${counted} counted, ${starved} skipped as starved (the machine or the runner was busy)`,
  ];
  urls.forEach((url, i) => {
    const s = snaps[i];
    lines.push(`page ${i}: ${url} -- mutations ${s?.mutations ?? "?"}, in flight: `
      + `${s && s.inflight.length ? s.inflight.join(", ") : "nothing"}`);
  });
  if (nodeBusy.size === 0) {
    lines.push("Node side: nothing declared busy. If the test was waiting on something outside the "
      + "page (a file, a daemon, a child process), wrap that wait in busy(promise, label).");
  }
  return lines.join("\n");
}

async function excerpt(context: BrowserContext): Promise<string> {
  const parts: string[] = [];
  for (const [i, page] of context.pages().entries()) {
    const text = await page.evaluate(() => (document.body?.innerText ?? "").replace(/\s+/g, " ")
      .trim().slice(0, 400)).catch(() => "(unreadable)");
    parts.push(`\npage ${i} text: ${text || "(empty)"}`);
  }
  return parts.join("");
}

export const test = base.extend<{ liveness: LivenessOptions }>({
  liveness: [LIVENESS_DEFAULTS, { option: true }],
  context: async ({ context, liveness }, use, testInfo) => {
    const watcher = await watchContext(context, testInfo, liveness);
    await use(context);
    await watcher.stop();
  },
});
