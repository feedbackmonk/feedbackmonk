// uldf-template: testing-liveness v1
// Vendored from <home>/templates/testing/playwright/ (testing-runtime TLV-2, DEC-745). Change the
// template and re-copy; the `test-liveness` oracle reports a copy older than the template.
//
// The page-side half of "is this test still making progress?": a counter of the page's work in
// flight and of DOM mutations. `installQuiescence` is passed to `addInitScript` as a FUNCTION, so
// Playwright serialises its source into every document before any app script runs. It must
// therefore close over nothing from this module.
//
// Counted automatically: every open `fetch` and `XMLHttpRequest`, and every pending `setTimeout`
// at or under `shortTimerMs` (a debounce or a deferred render is work about to happen; a longer
// timer or a `setInterval` is a schedule, and is not counted). Counted when the project says so:
// `window.__uldfQuiet.track(promise, label)` or `const end = window.__uldfQuiet.begin(label)`. A
// project wires its IPC or backend stub there, so a call into the stub counts as in flight until
// it settles.

export interface QuietSnapshot {
  /** A random id per document: a navigation or reload changes it. */
  doc: string;
  href: string;
  inflight: string[];
  mutations: number;
}

export interface QuietApi {
  begin(label: string): () => void;
  track<T>(work: T | PromiseLike<T>, label: string): T | PromiseLike<T>;
  snapshot(): QuietSnapshot;
  /** Resolves once nothing is in flight and two animation frames pass with no DOM mutation and
   *  nothing new in flight. It has no time limit: the stall detector ends a wait that cannot. */
  settled(): Promise<void>;
}

declare global {
  interface Window {
    __uldfQuiet?: QuietApi;
  }
}

export function installQuiescence(shortTimerMs: number): void {
  const w = window as any;
  if (w.__uldfQuiet) return;
  let seq = 0;
  const inflight = new Map<number, string>();
  let mutations = 0;
  let wake: Array<() => void> = [];
  const poke = () => {
    const due = wake;
    wake = [];
    for (const fn of due) fn();
  };
  const begin = (label: string) => {
    const id = ++seq;
    inflight.set(id, String(label));
    let open = true;
    return () => {
      if (!open) return;
      open = false;
      inflight.delete(id);
      poke();
    };
  };
  const track = (work: any, label: string) => {
    const end = begin(label);
    Promise.resolve(work).then(end, end);
    return work;
  };

  const realFetch = w.fetch;
  if (typeof realFetch === "function") {
    w.fetch = function (this: unknown, input: any, init?: any) {
      const url = typeof input === "string" ? input : (input && input.url) || String(input);
      const end = begin(`fetch ${url}`);
      try {
        const pending = realFetch.call(this, input, init);
        Promise.resolve(pending).then(end, end);
        return pending;
      } catch (error) {
        end();
        throw error;
      }
    };
  }

  const XHR = w.XMLHttpRequest;
  if (XHR && XHR.prototype) {
    const open = XHR.prototype.open;
    const send = XHR.prototype.send;
    XHR.prototype.open = function (this: any, method: string, url: string) {
      this.__uldfUrl = `${method} ${url}`;
      return open.apply(this, arguments as any);
    };
    XHR.prototype.send = function (this: any) {
      const end = begin(`xhr ${this.__uldfUrl ?? ""}`);
      this.addEventListener("loadend", end, { once: true });
      try {
        return send.apply(this, arguments as any);
      } catch (error) {
        end();
        throw error;
      }
    };
  }

  const realSetTimeout = w.setTimeout;
  const realClearTimeout = w.clearTimeout;
  const timers = new Map<unknown, () => void>();
  w.setTimeout = function (handler: any, ms?: number, ...rest: any[]) {
    const delay = Number(ms) || 0;
    if (typeof handler !== "function" || delay > shortTimerMs) {
      return realSetTimeout(handler, ms, ...rest);
    }
    const end = begin(`timer ${delay}ms`);
    const id = realSetTimeout(function (this: unknown) {
      timers.delete(id);
      try {
        return handler.apply(this, rest);
      } finally {
        end();
      }
    }, ms);
    timers.set(id, end);
    return id;
  };
  w.clearTimeout = function (id: unknown) {
    const end = timers.get(id);
    if (end) {
      timers.delete(id);
      end();
    }
    return realClearTimeout(id);
  };

  const observe = () => {
    new MutationObserver((records) => {
      mutations += records.length;
      poke();
    }).observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
  };
  if (document.documentElement) observe();
  else document.addEventListener("readystatechange", observe, { once: true });

  const doc = Math.random().toString(36).slice(2);
  const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  const change = () => new Promise<void>((resolve) => wake.push(resolve));

  const api: QuietApi = {
    begin,
    track,
    snapshot: () => ({ doc, href: location.href, inflight: [...inflight.values()], mutations }),
    async settled() {
      for (;;) {
        while (inflight.size > 0) await change();
        const seen = mutations;
        await frame();
        await frame();
        if (inflight.size === 0 && mutations === seen) return;
      }
    },
  };
  Object.defineProperty(w, "__uldfQuiet", { value: api, configurable: false });
}
