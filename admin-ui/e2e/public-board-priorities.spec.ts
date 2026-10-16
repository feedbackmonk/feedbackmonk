import { test, expect } from "./_liveness/liveness";
import type { Page, Route } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// End-user prioritization on the hosted board (Contract C43, FR-FBR-43/44/50).
//
// FAKE_API mode intercepts `/api/v1/projects/.../board…` and serves fixture
// JSON, so this runs without the Rust backend; the priority list is kept in
// the test so a PUT is answered with the order it saved, as the server does.
//
// What it proves, each with zero WCAG 2.1 AA axe violations:
//   1. a `signed_in` board read anonymously renders the sign-in prompt — not
//      the board-disabled "not available" state (FR-FBR-43);
//   2. the `#token=` handoff posts the token once and leaves no fragment in
//      the address bar or the history it lands on (FR-FBR-44);
//   3. a priority list is reordered by keyboard alone, every change saving the
//      whole order (FR-FBR-50);
//   4. a one-item list says it does not yet influence priority (FR-FBR-50).

const FAKE_API = process.env.PLAYWRIGHT_FAKE_API !== "0";

const PROJECT_ID = "00000000-0000-0000-0000-000000000abc";
const SIGNED_IN_ONLY_ID = "00000000-0000-0000-0000-00000051a1ed";
const TOKEN = "eyJhbGciOiJFZERTQSJ9.eyJzdWIiOiJ1LTEifQ.c2lnbmF0dXJl";

function item(code: string, body: string, status = "triaged") {
  return {
    short_code: code,
    body,
    kind: "feature",
    status,
    vote_count: 3,
    accepted_at: "2026-10-01T00:00:00Z",
  };
}

const POOL = [
  item("FB-AAA111", "Dark theme for the dashboard."),
  item("FB-BBB222", "Export to CSV.", "in-progress"),
  item("FB-CCC333", "Keyboard shortcuts everywhere."),
];

interface FakeBoard {
  order: string[];
  saves: string[][];
  sessionPosts: unknown[];
  signedIn: boolean;
}

function priorityBody(state: FakeBoard) {
  const entries = state.order.map((c) => POOL.find((p) => p.short_code === c)!);
  const unranked = POOL.filter((p) => !state.order.includes(p.short_code));
  return {
    entries,
    unranked,
    removed: [],
    removed_hidden: 0,
    influences_priority: entries.length >= 2,
  };
}

async function installFakeApi(page: Page, state: FakeBoard) {
  await page.route("**/api/v1/**", async (route: Route) => {
    const req = route.request();
    const url = req.url();
    const method = req.method();
    const json = (status: number, body: unknown) =>
      route.fulfill({
        status,
        contentType: "application/json",
        body: JSON.stringify(body),
      });

    // A `signed_in` board answers every anonymous read with SignInRequired.
    if (url.includes(`/projects/${SIGNED_IN_ONLY_ID}/board`)) {
      return json(401, { error: "SignInRequired" });
    }
    if (url.match(/\/board\/session(\?|$)/) && method === "POST") {
      state.sessionPosts.push(req.postDataJSON());
      state.signedIn = true;
      return json(200, { signed_in: true, expires_at: "2026-10-14T02:00:00Z" });
    }
    if (url.match(/\/board\/priorities(\?|$)/)) {
      if (method === "PUT") {
        const body = req.postDataJSON() as { items: string[] };
        state.saves.push(body.items);
        state.order = body.items;
      }
      return json(200, priorityBody(state));
    }
    if (url.match(/\/projects\/[^/]+\/board(\?|$)/) && method === "GET") {
      return json(200, {
        items: POOL.map((p) =>
          state.signedIn
            ? {
                ...p,
                viewer: {
                  upvoted: false,
                  not_needed: false,
                  priority_position: state.order.includes(p.short_code)
                    ? state.order.indexOf(p.short_code)
                    : null,
                },
              }
            : p,
        ),
        total: POOL.length,
        limit: 50,
        offset: 0,
        access: "public",
        signed_in: state.signedIn,
      });
    }
    return route.fallback();
  });
}

function freshState(order: string[] = []): FakeBoard {
  return { order, saves: [], sessionPosts: [], signedIn: true };
}

async function expectNoAxeViolations(page: Page, label: string) {
  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
    .analyze();
  expect(results.violations, `axe violations on ${label}`).toEqual([]);
}

async function rankedCodes(page: Page): Promise<string[]> {
  return page
    .locator("section", { has: page.getByRole("heading", { name: "Your order" }) })
    .locator("ol > li a")
    .allTextContents();
}

test.describe("Board end-user prioritization", () => {
  test.use({ locale: "en-US" });

  test("a signed_in board renders the sign-in prompt, not 'not available' (FR-FBR-43)", async ({
    page,
  }) => {
    test.skip(!FAKE_API, "Real-backend mode requires a seeded signed_in project");
    await installFakeApi(page, freshState());

    await page.goto(`/public/projects/${SIGNED_IN_ONLY_ID}/board`);
    await expect(
      page.getByRole("heading", { name: "Sign in to see this board" }),
    ).toBeVisible();
    await expect(page.getByText(/this feedback board isn’t available/i)).toHaveCount(0);
    await expectNoAxeViolations(page, "signed_in board sign-in prompt");
  });

  test("the #token= handoff posts once and leaves no fragment behind (FR-FBR-44)", async ({
    page,
  }) => {
    test.skip(!FAKE_API, "Real-backend mode requires a customer-signed JWT");
    const state = freshState();
    state.signedIn = false;
    await installFakeApi(page, state);

    await page.goto(`/public/projects/${PROJECT_ID}/board/sign-in#token=${TOKEN}`);

    // Lands on the board, signed in.
    await expect(page).toHaveURL(new RegExp(`/public/projects/${PROJECT_ID}/board$`));
    await expect(page.getByRole("link", { name: "My priorities" })).toBeVisible();

    expect(state.sessionPosts).toEqual([{ token: TOKEN }]);
    expect(page.url()).not.toContain("#");
    expect(page.url()).not.toContain(TOKEN);
    expect(await page.evaluate(() => window.location.hash)).toBe("");

    // The sign-in entry was replaced, not pushed: Back does not return to a
    // URL carrying the token.
    await page.goBack();
    expect(page.url()).not.toContain(TOKEN);

    await page.goto(`/public/projects/${PROJECT_ID}/board`);
    await expect(page.getByRole("link", { name: "My priorities" })).toBeVisible();
    await expectNoAxeViolations(page, "signed-in board");
  });

  test("a priority list is reordered by keyboard alone, saving the whole order (FR-FBR-50)", async ({
    page,
  }) => {
    test.skip(!FAKE_API, "Real-backend mode requires a signed-in viewer");
    const state = freshState(["FB-AAA111", "FB-BBB222", "FB-CCC333"]);
    await installFakeApi(page, state);

    await page.goto(`/public/projects/${PROJECT_ID}/board/priorities`);
    await expect(page.getByRole("heading", { name: "My priorities", level: 1 })).toBeVisible();
    expect(await rankedCodes(page)).toEqual(["FB-AAA111", "FB-BBB222", "FB-CCC333"]);
    await expectNoAxeViolations(page, "priorities, three items");

    // Reach the first item's Move down button with the Tab key only.
    const moveDownA = page.getByRole("button", { name: "Move down: FB-AAA111" });
    for (let i = 0; i < 40; i++) {
      if (await moveDownA.evaluate((el) => el === document.activeElement)) break;
      await page.keyboard.press("Tab");
    }
    await expect(moveDownA).toBeFocused();

    await page.keyboard.press("Enter");
    await expect.poll(() => rankedCodes(page)).toEqual([
      "FB-BBB222",
      "FB-AAA111",
      "FB-CCC333",
    ]);
    // Focus followed the moved item, so the next key press moves it again.
    await expect(moveDownA).toBeFocused();

    await page.keyboard.press("Space");
    await expect.poll(() => rankedCodes(page)).toEqual([
      "FB-BBB222",
      "FB-CCC333",
      "FB-AAA111",
    ]);
    // At the bottom, Move down is disabled and focus moved to Move up.
    await expect(moveDownA).toBeDisabled();
    await expect(page.getByRole("button", { name: "Move up: FB-AAA111" })).toBeFocused();

    // Every change saved the WHOLE order, top first.
    await expect.poll(() => state.saves).toEqual([
      ["FB-BBB222", "FB-AAA111", "FB-CCC333"],
      ["FB-BBB222", "FB-CCC333", "FB-AAA111"],
    ]);
    await expect(page.getByRole("status").filter({ hasText: "FB-AAA111 is now number 3 of 3." })).toHaveCount(1);
    await expectNoAxeViolations(page, "priorities after keyboard reorder");
  });

  test("a one-item list says it does not yet influence priority (FR-FBR-50)", async ({
    page,
  }) => {
    test.skip(!FAKE_API, "Real-backend mode requires a signed-in viewer");
    await installFakeApi(page, freshState(["FB-BBB222"]));

    await page.goto(`/public/projects/${PROJECT_ID}/board/priorities`);
    await expect(
      page.getByText("Rank at least two requests for your list to influence priority."),
    ).toBeVisible();
    expect(await rankedCodes(page)).toEqual(["FB-BBB222"]);
    await expect(page.getByRole("heading", { name: "Not yet ranked" })).toBeVisible();
    await expectNoAxeViolations(page, "priorities, one item");
  });
});
