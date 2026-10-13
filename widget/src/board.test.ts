import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { boardLink, fetchBoardAccess } from "./board.js";
import { mountFeedbackMonk } from "./widget.js";
import type { FeedbackMonkHandle } from "./types.js";

// FR-FBR-45: the widget's "See all requests" link to the hosted board.
//
//   - shown when the board is enabled AND the widget holds a user token, and
//     then it performs the FR-FBR-44 handoff: the token rides in the URL
//     FRAGMENT of the sign-in page, never in the query or the path;
//   - without a token it opens the board unsigned — and is NOT shown on a
//     `signed_in` board, where an unsigned visitor could only meet a sign-in
//     wall;
//   - never shown when the board is disabled (the board answers 404).

const PROJECT = "00000000-0000-0000-0000-000000000001";
const API = "https://api.example.test";
// header.payload.signature — the payload carries an email so the modal's
// auth-mode reply line has something to show.
const JWT =
  "eyJhbGciOiJFZERTQSJ9." +
  btoa(JSON.stringify({ sub: "u-1", email: "a@example.test" }))
    .replace(/=+$/, "")
    .replace(/\+/g, "-")
    .replace(/\//g, "_") +
  ".c2ln";

const CONFIG = {
  project_id: PROJECT,
  tenant_id: "00000000-0000-0000-0000-000000000002",
  display_name: "Fixture Project",
  brand: {
    primary_color: null,
    logo_url: null,
    footer_text: null,
    footer_url: null,
    theme: null,
  },
  auth_modes: ["auth", "anonymous"],
  submission_kinds: ["bug", "feature", "question", "other"],
  max_body_chars: 16384,
};

type BoardAnswer = "public" | "signed_in" | "disabled" | "network";

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

/// A fetch double: widget-config, the board probe, and an optional host-page
/// token endpoint. Every call is recorded so a test can read the wire.
function installFetch(board: BoardAnswer, tokenFromUrl?: string) {
  const calls: { url: string; init: RequestInit | undefined }[] = [];
  const fn = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    calls.push({ url, init });
    if (url.endsWith("/widget-config")) return json(200, CONFIG);
    if (url.includes("/board")) {
      if (board === "network") throw new TypeError("network down");
      if (board === "disabled") return json(404, { error: "not found" });
      if (board === "signed_in") return json(401, { error: "SignInRequired" });
      return json(200, {
        items: [],
        total: 0,
        limit: 1,
        offset: 0,
        access: "public",
        signed_in: false,
      });
    }
    if (url === "/token") {
      return json(200, tokenFromUrl ? { token: tokenFromUrl, expires_at: 4102444800 } : {});
    }
    return json(404, {});
  });
  vi.stubGlobal("fetch", fn);
  return calls;
}

async function settle(): Promise<void> {
  for (let i = 0; i < 10; i++) await new Promise((r) => setTimeout(r, 0));
}

let handle: FeedbackMonkHandle | undefined;

async function openWith(
  board: BoardAnswer,
  opts: { jwt?: string; tokenUrl?: string; boardUrl?: string },
  tokenFromUrl?: string,
) {
  const calls = installFetch(board, tokenFromUrl);
  handle = await mountFeedbackMonk({ projectId: PROJECT, apiBase: API, ...opts });
  expect(handle).toBeDefined();
  handle!.open();
  await settle();
  expect(document.querySelector("[role=dialog]")).not.toBeNull();
  const link = document.querySelector<HTMLAnchorElement>(".fbm-board a");
  return { link, calls };
}

beforeEach(() => {
  document.body.replaceChildren();
});

afterEach(() => {
  handle?.destroy();
  handle = undefined;
  vi.unstubAllGlobals();
});

describe("boardLink (FR-FBR-45)", () => {
  it("carries the token in the fragment of the sign-in page, never the query", () => {
    const href = boardLink(API, PROJECT, "public", JWT)!;
    const url = new URL(href);
    expect(url.origin).toBe(API);
    expect(url.pathname).toBe(`/public/projects/${PROJECT}/board/sign-in`);
    expect(url.search).toBe("");
    expect(url.hash).toBe("#token=" + JWT);
    expect(url.pathname).not.toContain(JWT);
  });

  it("links a token holder on a signed_in board through the same handoff", () => {
    const url = new URL(boardLink(API, PROJECT, "signed_in", JWT)!);
    expect(url.pathname).toBe(`/public/projects/${PROJECT}/board/sign-in`);
    expect(url.hash).toBe("#token=" + JWT);
  });

  it("is absent on a signed_in board without a token", () => {
    expect(boardLink(API, PROJECT, "signed_in", undefined)).toBeNull();
  });

  it("opens a public board unsigned without a token", () => {
    const url = new URL(boardLink(API, PROJECT, "public", undefined)!);
    expect(url.pathname).toBe(`/public/projects/${PROJECT}/board`);
    expect(url.hash).toBe("");
    expect(url.search).toBe("");
  });

  it("is absent when the board is disabled, token or not", () => {
    expect(boardLink(API, PROJECT, null, JWT)).toBeNull();
    expect(boardLink(API, PROJECT, null, undefined)).toBeNull();
  });

  it("refuses a board base that is not http(s)", () => {
    expect(boardLink("javascript:alert(1)//", PROJECT, "public", JWT)).toBeNull();
    expect(boardLink(undefined, PROJECT, "public", JWT)).toBeNull();
  });
});

describe("fetchBoardAccess", () => {
  it("reads 200 as public, 401 SignInRequired as signed_in, anything else as disabled", async () => {
    installFetch("public");
    expect(await fetchBoardAccess(PROJECT, { apiBase: API })).toBe("public");
    installFetch("signed_in");
    expect(await fetchBoardAccess(PROJECT, { apiBase: API })).toBe("signed_in");
    installFetch("disabled");
    expect(await fetchBoardAccess(PROJECT, { apiBase: API })).toBeNull();
    installFetch("network");
    expect(await fetchBoardAccess(PROJECT, { apiBase: API })).toBeNull();
  });

  it("never sends the token or a cookie on the probe", async () => {
    const calls = installFetch("public");
    await fetchBoardAccess(PROJECT, { apiBase: API, jwt: JWT });
    expect(calls).toHaveLength(1);
    const { url, init } = calls[0];
    expect(url).toBe(`${API}/api/v1/projects/${PROJECT}/board?limit=1`);
    expect(url).not.toContain(JWT);
    expect(init?.credentials).toBe("omit");
    expect(JSON.stringify(init?.headers ?? {})).not.toContain(JWT);
  });
});

describe("the link in the open widget (FR-FBR-45)", () => {
  it("is present with a data-jwt token and an enabled public board", async () => {
    const { link } = await openWith("public", { jwt: JWT });
    expect(link).not.toBeNull();
    expect(link!.textContent).toBe("See all requests");
    const url = new URL(link!.href);
    expect(url.pathname).toBe(`/public/projects/${PROJECT}/board/sign-in`);
    expect(url.search).toBe("");
    expect(url.hash).toBe("#token=" + JWT);
    expect(link!.target).toBe("_blank");
    expect(link!.rel).toContain("noopener");
    expect(link!.rel).toContain("noreferrer");
  });

  it("is present with a token on a signed_in board", async () => {
    const { link } = await openWith("signed_in", { jwt: JWT });
    expect(link).not.toBeNull();
    expect(new URL(link!.href).hash).toBe("#token=" + JWT);
  });

  it("carries a token fetched from data-token-url, in the fragment", async () => {
    const { link } = await openWith("public", { tokenUrl: "/token" }, JWT);
    expect(link).not.toBeNull();
    const url = new URL(link!.href);
    expect(url.search).toBe("");
    expect(url.hash).toBe("#token=" + JWT);
  });

  it("is absent on a signed_in board without a token", async () => {
    const { link } = await openWith("signed_in", {});
    expect(link).toBeNull();
  });

  it("is absent on a signed_in board when data-token-url answers signed out", async () => {
    const { link } = await openWith("signed_in", { tokenUrl: "/token" });
    expect(link).toBeNull();
  });

  it("is absent when the board is disabled, even with a token", async () => {
    const { link } = await openWith("disabled", { jwt: JWT });
    expect(link).toBeNull();
  });

  it("opens a public board unsigned when there is no token", async () => {
    const { link } = await openWith("public", {});
    expect(link).not.toBeNull();
    const url = new URL(link!.href);
    expect(url.pathname).toBe(`/public/projects/${PROJECT}/board`);
    expect(url.hash).toBe("");
  });

  it("points at data-board-url when the board is served elsewhere", async () => {
    const { link } = await openWith("public", {
      jwt: JWT,
      boardUrl: "https://board.example.test/",
    });
    const url = new URL(link!.href);
    expect(url.origin).toBe("https://board.example.test");
    expect(url.pathname).toBe(`/public/projects/${PROJECT}/board/sign-in`);
  });

  it("never puts the token on any request the widget makes to the board", async () => {
    const { calls } = await openWith("public", { jwt: JWT });
    const probes = calls.filter((c) => c.url.includes("/board"));
    expect(probes.length).toBeGreaterThan(0);
    for (const c of probes) {
      expect(c.url).not.toContain(JWT);
      expect(JSON.stringify(c.init?.headers ?? {})).not.toContain(JWT);
    }
  });
});
