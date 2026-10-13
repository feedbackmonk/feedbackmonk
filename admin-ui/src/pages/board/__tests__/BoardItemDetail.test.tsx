import { describe, expect, it, vi, beforeEach } from "vitest";
import axios from "axios";
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { BoardItemDetail } from "../BoardItemDetail";
import { ToastProvider } from "../../../components/Toast";
import { renderWithClient } from "../../../test/testUtils";
import type { BoardItemDetail as Detail } from "../../../shared/types.gen";

vi.mock("../../../shared/ApiClient", () => ({
  fetchBoardItemDetail: vi.fn(),
  castBoardVote: vi.fn(),
  retractBoardVote: vi.fn(),
  markNotNeeded: vi.fn(),
  clearNotNeeded: vi.fn(),
  fetchMyPriorities: vi.fn(),
  saveMyPriorities: vi.fn(),
}));

import {
  fetchBoardItemDetail,
  fetchMyPriorities,
  markNotNeeded,
  saveMyPriorities,
} from "../../../shared/ApiClient";
const mockedDetail = vi.mocked(fetchBoardItemDetail);
const mockedMark = vi.mocked(markNotNeeded);
const mockedFetchList = vi.mocked(fetchMyPriorities);
const mockedSaveList = vi.mocked(saveMyPriorities);

const PROJECT_ID = "00000000-0000-0000-0000-000000000abc";
const CODE = "FB-AAA111";

const base: Detail = {
  short_code: CODE,
  body: "Bitte einen dunklen Modus.",
  kind: "feature",
  status: "in-progress",
  vote_count: 12,
  accepted_at: "2026-10-01T00:00:00Z",
  board_note: {
    body: "We're starting this in November.",
    updated_at: "2026-10-10T00:00:00Z",
  },
  timeline: [
    { status: "triaged", at: "2026-10-02T00:00:00Z" },
    { status: "in-progress", at: "2026-10-05T00:00:00Z" },
  ],
  in_ranking_pool: true,
};

function renderPage() {
  return renderWithClient(
    <ToastProvider>
      <BoardItemDetail projectId={PROJECT_ID} shortCode={CODE} />
    </ToastProvider>,
    {
      withRouter: true,
      initialPath: `/public/projects/${PROJECT_ID}/board/items/${CODE}`,
    },
  );
}

describe("BoardItemDetail (FR-FBR-57/58)", () => {
  beforeEach(() => {
    mockedDetail.mockReset();
    mockedMark.mockReset();
    mockedFetchList.mockReset();
    mockedSaveList.mockReset();
  });

  it("shows the body verbatim, kind, status, votes, the board note and the status timeline", async () => {
    mockedDetail.mockResolvedValueOnce(base);
    renderPage();

    expect(await screen.findByText("Bitte einen dunklen Modus.")).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: /Feature · FB-AAA111/, level: 1 }),
    ).toBeInTheDocument();
    expect(screen.getByText("We're starting this in November.")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Status history" })).toBeInTheDocument();
    const timeline = screen
      .getByRole("heading", { name: "Status history" })
      .parentElement!.querySelectorAll("li");
    expect(timeline).toHaveLength(2);
    expect(
      screen.getByRole("button", { name: /Vote for FB-AAA111 — current count 12/ }),
    ).toBeInTheDocument();
  });

  it("offers no not-needed or priority action to an anonymous viewer", async () => {
    mockedDetail.mockResolvedValueOnce(base);
    renderPage();

    await screen.findByText("Bitte einen dunklen Modus.");
    expect(
      screen.queryByRole("button", { name: "I don’t need this" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Add to my priorities" }),
    ).not.toBeInTheDocument();
  });

  it("lets a signed-in viewer mark a pool item not needed (FR-FBR-48)", async () => {
    mockedDetail.mockResolvedValue({
      ...base,
      viewer: { upvoted: true, not_needed: false, priority_position: null },
    });
    mockedMark.mockResolvedValueOnce({
      short_code: CODE,
      not_needed: true,
      upvote_cleared: true,
    });
    renderPage();

    const btn = await screen.findByRole("button", { name: "I don’t need this" });
    expect(btn).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(btn);
    await waitFor(() => {
      expect(mockedMark).toHaveBeenCalledWith(PROJECT_ID, CODE);
    });
  });

  it("adds the item to the end of the viewer's whole list", async () => {
    mockedDetail.mockResolvedValue({
      ...base,
      viewer: { upvoted: false, not_needed: false, priority_position: null },
    });
    const other = { ...base, short_code: "FB-ZZZ999", body: "Other." };
    mockedFetchList.mockResolvedValueOnce({
      entries: [other],
      unranked: [],
      removed: [],
      removed_hidden: 0,
      influences_priority: false,
    });
    mockedSaveList.mockResolvedValueOnce({
      entries: [other, base],
      unranked: [],
      removed: [],
      removed_hidden: 0,
      influences_priority: true,
    });
    renderPage();

    fireEvent.click(
      await screen.findByRole("button", { name: "Add to my priorities" }),
    );
    await waitFor(() => {
      expect(mockedSaveList).toHaveBeenCalledWith(PROJECT_ID, ["FB-ZZZ999", CODE]);
    });
  });

  it("offers no not-needed action for an item outside the ranking pool", async () => {
    mockedDetail.mockResolvedValueOnce({
      ...base,
      status: "shipped",
      in_ranking_pool: false,
      viewer: { upvoted: false, not_needed: false, priority_position: null },
    });
    renderPage();

    await screen.findByText("Bitte einen dunklen Modus.");
    expect(
      screen.queryByRole("button", { name: "I don’t need this" }),
    ).not.toBeInTheDocument();
  });

  it("never renders a submitter-identity field a malformed payload smuggles (FR-FBR-59)", async () => {
    // None of these fields is part of the C43 detail shape.
    const leaky = {
      ...base,
      end_user_email: "leaked@example.com",
      replies: [{ body: "internal reply text" }],
      body_translated: "Please add a dark mode.",
    } as unknown as Detail;
    mockedDetail.mockResolvedValueOnce(leaky);
    const { container } = renderPage();

    await screen.findByText("Bitte einen dunklen Modus.");
    expect(container.textContent).not.toContain("leaked@example.com");
    expect(container.textContent).not.toContain("internal reply text");
    expect(container.textContent).not.toContain("Please add a dark mode.");
  });

  it("renders a not-on-the-board state for a 404", async () => {
    const err = new axios.AxiosError("not found");
    // @ts-expect-error — minimal AxiosResponse stub.
    err.response = { status: 404, data: {} };
    mockedDetail.mockRejectedValue(err);
    renderPage();

    expect(
      await screen.findByText("This item isn’t on the feedback board."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
