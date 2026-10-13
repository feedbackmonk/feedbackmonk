import { describe, expect, it, vi, beforeEach } from "vitest";
import axios from "axios";
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { MyPriorities } from "../MyPriorities";
import { ToastProvider } from "../../../components/Toast";
import { renderWithClient } from "../../../test/testUtils";
import type { BoardItem, PriorityListResponse } from "../../../shared/types.gen";

vi.mock("../../../shared/ApiClient", () => ({
  fetchMyPriorities: vi.fn(),
  saveMyPriorities: vi.fn(),
}));

import { fetchMyPriorities, saveMyPriorities } from "../../../shared/ApiClient";
const mockedFetch = vi.mocked(fetchMyPriorities);
const mockedSave = vi.mocked(saveMyPriorities);

const PROJECT_ID = "00000000-0000-0000-0000-000000000abc";

function item(code: string, body: string): BoardItem {
  return {
    short_code: code,
    body,
    kind: "feature",
    status: "triaged",
    vote_count: 1,
    accepted_at: "2026-10-01T00:00:00Z",
  };
}

const A = item("FB-AAA111", "Dark theme for the dashboard.");
const B = item("FB-BBB222", "Export to CSV.");
const C = item("FB-CCC333", "Keyboard shortcuts everywhere.");

function list(
  entries: BoardItem[],
  unranked: BoardItem[],
  extra: Partial<PriorityListResponse> = {},
): PriorityListResponse {
  return {
    entries,
    unranked,
    removed: [],
    removed_hidden: 0,
    influences_priority: entries.length >= 2,
    ...extra,
  };
}

// The server echoes the saved order back; the mock does the same.
function echoSave() {
  mockedSave.mockImplementation(async (_pid, codes) => {
    const all = [A, B, C];
    const entries = codes.map((c) => all.find((x) => x.short_code === c)!);
    const unranked = all.filter((x) => !codes.includes(x.short_code));
    return list(entries, unranked);
  });
}

function renderPage() {
  return renderWithClient(
    <ToastProvider>
      <MyPriorities projectId={PROJECT_ID} />
    </ToastProvider>,
    {
      withRouter: true,
      initialPath: `/public/projects/${PROJECT_ID}/board/priorities`,
    },
  );
}

function rankedCodes(): string[] {
  const listEl = screen
    .getByRole("heading", { name: "Your order" })
    .parentElement!.querySelector("ol")!;
  return Array.from(listEl.querySelectorAll(":scope > li")).map(
    (li) => li.querySelector("a")!.textContent!,
  );
}

describe("MyPriorities", () => {
  beforeEach(() => {
    mockedFetch.mockReset();
    mockedSave.mockReset();
  });

  it("renders the viewer's order and the not-yet-ranked pool items separately", async () => {
    mockedFetch.mockResolvedValueOnce(list([A, B], [C]));
    renderPage();

    await screen.findByText("Dark theme for the dashboard.");
    expect(rankedCodes()).toEqual(["FB-AAA111", "FB-BBB222"]);
    expect(
      screen.getByRole("heading", { name: "Not yet ranked" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Add: FB-CCC333" }),
    ).toBeInTheDocument();
    // Two items: no "does not yet influence" note.
    expect(
      screen.queryByText(/does not yet|at least two requests/i),
    ).not.toBeInTheDocument();
  });

  it("moves an item down and saves the WHOLE order (FR-FBR-50), keeping focus on the moved item", async () => {
    mockedFetch.mockResolvedValueOnce(list([A, B, C], []));
    echoSave();
    renderPage();

    const down = await screen.findByRole("button", {
      name: "Move down: FB-AAA111",
    });
    down.focus();
    fireEvent.click(down);

    await waitFor(() => {
      expect(mockedSave).toHaveBeenCalledWith(PROJECT_ID, [
        "FB-BBB222",
        "FB-AAA111",
        "FB-CCC333",
      ]);
    });
    expect(rankedCodes()).toEqual(["FB-BBB222", "FB-AAA111", "FB-CCC333"]);
    // Focus follows the moved item, so a keyboard user can keep moving it.
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Move down: FB-AAA111" }),
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "FB-AAA111 is now number 2 of 3.",
    );
  });

  it("hands focus to Move down when an item reaches the top", async () => {
    mockedFetch.mockResolvedValueOnce(list([A, B], []));
    echoSave();
    renderPage();

    const up = await screen.findByRole("button", { name: "Move up: FB-BBB222" });
    up.focus();
    fireEvent.click(up);

    await waitFor(() => {
      expect(rankedCodes()).toEqual(["FB-BBB222", "FB-AAA111"]);
    });
    expect(
      screen.getByRole("button", { name: "Move up: FB-BBB222" }),
    ).toBeDisabled();
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "Move down: FB-BBB222" }),
    );
  });

  it("adds a not-yet-ranked item to the end of the list", async () => {
    mockedFetch.mockResolvedValueOnce(list([A], [B, C]));
    echoSave();
    renderPage();

    fireEvent.click(await screen.findByRole("button", { name: "Add: FB-CCC333" }));
    await waitFor(() => {
      expect(mockedSave).toHaveBeenCalledWith(PROJECT_ID, [
        "FB-AAA111",
        "FB-CCC333",
      ]);
    });
    expect(rankedCodes()).toEqual(["FB-AAA111", "FB-CCC333"]);
  });

  it("says a one-item list does not yet influence priority (FR-FBR-50)", async () => {
    mockedFetch.mockResolvedValueOnce(list([A], [B]));
    renderPage();

    await screen.findByText("Dark theme for the dashboard.");
    expect(
      screen.getByText(
        "Rank at least two requests for your list to influence priority.",
      ),
    ).toBeInTheDocument();
  });

  it("removes an item and saves the shorter list", async () => {
    mockedFetch.mockResolvedValueOnce(list([A, B], []));
    echoSave();
    renderPage();

    fireEvent.click(
      await screen.findByRole("button", { name: "Remove: FB-AAA111" }),
    );
    await waitFor(() => {
      expect(mockedSave).toHaveBeenCalledWith(PROJECT_ID, ["FB-BBB222"]);
    });
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Add: FB-AAA111" }),
      ).toBeInTheDocument();
    });
  });

  it("reports items that left the ranking pool once (FR-FBR-51)", async () => {
    mockedFetch.mockResolvedValueOnce(
      list([A], [], {
        removed: [item("FB-OLD999", "Shipped already.")],
        removed_hidden: 2,
      }),
    );
    renderPage();

    await screen.findByText("Shipped already.");
    expect(
      screen.getByText(
        "1 request is no longer open, so it was taken off your list:",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "2 requests are no longer on the board and were taken off your list.",
      ),
    ).toBeInTheDocument();
  });

  it("renders the sign-in prompt for an anonymous viewer (401 SignInRequired)", async () => {
    const err = new axios.AxiosError("unauthorized");
    // @ts-expect-error — minimal AxiosResponse stub.
    err.response = { status: 401, data: { error: "SignInRequired" } };
    mockedFetch.mockRejectedValue(err);
    renderPage();

    expect(
      await screen.findByRole("heading", { name: "Sign in to see this board" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
