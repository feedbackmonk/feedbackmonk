import { describe, expect, it } from "vitest";

import { isAdminPath } from "./useAdminLocaleDefault";

// Which paths receive the tenant's configured locale as a default (Contract
// C38, precedence rung 3).
//
// This exists because the answer regressed once: `/feedback` — the console's
// busiest screen, and not under `/admin` only for historical URL reasons — was
// excluded, so the whole document's `lang` AND `dir` flipped when the operator
// navigated to any `/admin/*` page and reverted on reload, inside one tab
// (R-A11Y finding A-1, collab-20260907-034037). The gate itself is correct and
// load-bearing: `shared/ApiClient`'s 401 interceptor would bounce an anonymous
// visitor off a public page, so the public surfaces and `/login` must stay out.

describe("isAdminPath — the authenticated console, and nothing else", () => {
  it("covers every /admin route", () => {
    for (const p of [
      "/admin",
      "/admin/",
      "/admin/roadmap",
      "/admin/settings/language",
      "/admin/moderation",
      "/admin/autopilot/work-orders/WO-1",
    ]) {
      expect(isAdminPath(p), p).toBe(true);
    }
  });

  it("covers /feedback and /feedback/{id} (A-1)", () => {
    expect(isAdminPath("/feedback")).toBe(true);
    expect(isAdminPath("/feedback/")).toBe(true);
    expect(isAdminPath("/feedback/FB-ABC123")).toBe(true);
  });

  it("excludes /login and every public surface", () => {
    // Each of these can be reached by an anonymous visitor; asking for the
    // tenant's locale there would 401 and redirect them off the page.
    for (const p of [
      "/login",
      "/",
      "/board",
      "/roadmap",
      "/public/projects/11111111-1111-1111-1111-111111111111/board",
      "/public/projects/11111111-1111-1111-1111-111111111111/roadmap",
    ]) {
      expect(isAdminPath(p), p).toBe(false);
    }
  });

  it("does not match a path that merely starts with the same letters", () => {
    expect(isAdminPath("/feedbackomatic")).toBe(false);
    expect(isAdminPath("/administrivia")).toBe(false);
  });
});
