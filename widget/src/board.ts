import type { MountOptions } from "./types.js";
import { resolveApiBase } from "./api.js";

// The "See all requests" link to the hosted public board (FR-FBR-45).
//
// The widget learns whether the board is on, and who may read it, by asking the
// board itself — one anonymous `GET …/board?limit=1` with no token and no
// cookie (Contract C43): 200 ⇒ an enabled public board, 401 `SignInRequired`
// ⇒ an enabled `signed_in` board, anything else (404 = disabled, an error, the
// network) ⇒ no link. The user's token never travels on that probe.
//
// The token reaches the board only in the URL FRAGMENT of its sign-in page
// (FR-FBR-44): a fragment is never sent to a server, never logged by one, and
// never carried in a Referer, and the board page clears it after the exchange.

export type BoardAccess = "public" | "signed_in";

export async function fetchBoardAccess(
  projectId: string,
  opts: MountOptions,
): Promise<BoardAccess | null> {
  try {
    const r = await fetch(
      resolveApiBase(opts) +
        "/api/v1/projects/" +
        encodeURIComponent(projectId) +
        "/board?limit=1",
      { headers: { Accept: "application/json" }, credentials: "omit" },
    );
    if (r.ok) return "public";
    if (r.status === 401) {
      const d = (await r.json()) as { error?: unknown };
      if (d && d.error === "SignInRequired") return "signed_in";
    }
  } catch {
    // no board link
  }
  return null;
}

/// The board link for this viewer, or null when none is shown: the board is
/// off, or it is `signed_in` and the widget holds no token. A token holder goes
/// through the sign-in handoff with the token in the fragment; anyone else
/// opens the public board unsigned. `boardBase` must be http(s).
export function boardLink(
  boardBase: string | undefined,
  projectId: string,
  access: BoardAccess | null,
  jwt: string | undefined,
): string | null {
  if (!access || !boardBase || (!jwt && access === "signed_in")) return null;
  let base: URL;
  try {
    base = new URL(boardBase);
  } catch {
    return null;
  }
  if (base.protocol !== "https:" && base.protocol !== "http:") return null;
  const board =
    (base.origin + base.pathname).replace(/\/$/, "") + "/public/projects/" + encodeURIComponent(projectId) + "/board";
  return jwt ? board + "/sign-in#token=" + encodeURIComponent(jwt) : board;
}
