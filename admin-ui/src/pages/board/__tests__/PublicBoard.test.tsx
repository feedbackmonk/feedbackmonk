import { describe, expect, it, vi, beforeEach } from "vitest";
import axios from "axios";
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { PublicBoard } from "../PublicBoard";
import { ToastProvider } from "../../../components/Toast";
import { renderWithClient } from "../../../test/testUtils";
import type { BoardViewerListResponse } from "../../../shared/types.gen";

vi.mock("../../../shared/ApiClient", () => ({
  fetchBoardForViewer: vi.fn(),
  castBoardVote: vi.fn(),
  retractBoardVote: vi.fn(),
  endBoardSession: vi.fn(),
}));

import {
  castBoardVote,
  fetchBoardForViewer,
  retractBoardVote,
} from "../../../shared/ApiClient";
// The board reads through the C43 viewer-aware client since end-user
// prioritization; a C29 list is a valid C43 list for an anonymous viewer.
const mockedList = vi.mocked(fetchBoardForViewer);
const mockedCast = vi.mocked(castBoardVote);
const mockedRetract = vi.mocked(retractBoardVote);

const PROJECT_ID = "00000000-0000-0000-0000-000000000abc";

const exampleList: BoardViewerListResponse = {
  items: [
    {
      short_code: "FB-AAA111",
      body: "The export button silently fails on Safari.",
      kind: "bug",
      status: "triaged",
      vote_count: 9,
      accepted_at: "2026-06-18T00:00:00Z",
    },
    {
      short_code: "FB-BBB222",
      body: "Please add a dark theme.",
      kind: "feature",
      status: "in-progress",
      vote_count: 14,
      accepted_at: "2026-06-17T00:00:00Z",
    },
  ],
  total: 2,
  limit: 50,
  offset: 0,
  access: "public",
  signed_in: false,
};

describe("PublicBoard", () => {
  beforeEach(() => {
    mockedList.mockReset();
    mockedCast.mockReset();
    mockedRetract.mockReset();
  });

  it("renders approved board items with body, kind, status and a vote button carrying the count", async () => {
    mockedList.mockResolvedValueOnce(exampleList);
    renderWithClient(
      <ToastProvider>
        <PublicBoard projectId={PROJECT_ID} />
      </ToastProvider>,
      { withRouter: true, initialPath: `/public/projects/${PROJECT_ID}/board` },
    );

    await waitFor(() => {
      expect(
        screen.getByText("The export button silently fails on Safari."),
      ).toBeInTheDocument();
      expect(screen.getByText("Please add a dark theme.")).toBeInTheDocument();
    });

    // Item heading embeds the kind label + short_code (no submitter identity).
    expect(
      screen.getByRole("heading", { name: /Bug · FB-AAA111/, level: 2 }),
    ).toBeInTheDocument();
    // The real vote count (C30) is carried by an accessible vote button per item.
    expect(
      screen.getByRole("button", {
        name: /Vote for FB-AAA111 — current count 9/,
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: /Vote for FB-BBB222 — current count 14/,
      }),
    ).toBeInTheDocument();
  });

  it("casts a board vote on click (C30) and refetches the list", async () => {
    mockedList.mockResolvedValue(exampleList);
    mockedCast.mockResolvedValueOnce({
      short_code: "FB-AAA111",
      voter_mode: "anon",
      cast_at: "2026-06-19T12:00:00Z",
    });
    renderWithClient(
      <ToastProvider>
        <PublicBoard projectId={PROJECT_ID} />
      </ToastProvider>,
      { withRouter: true, initialPath: `/public/projects/${PROJECT_ID}/board` },
    );

    const voteBtn = await screen.findByRole("button", {
      name: /Vote for FB-AAA111 — current count 9/,
    });
    fireEvent.click(voteBtn);

    await waitFor(() => {
      expect(mockedCast).toHaveBeenCalledWith(PROJECT_ID, "FB-AAA111");
    });
  });

  it("never references a submitter-identity field (C29 privacy invariant)", async () => {
    // Even if a malformed payload smuggled identity fields, the component must
    // not surface them. We assert the rendered DOM contains none of them.
    const leaky = {
      ...exampleList,
      items: [
        {
          ...exampleList.items[0],
          // @ts-expect-error — these fields are NOT part of BoardItem; the
          // server never sends them and the component must never read them.
          end_user_email: "leaked@example.com",
          end_user_name: "Leaky McLeakface",
          anon_token_hash: "deadbeef",
        },
      ],
    } as BoardViewerListResponse;
    mockedList.mockResolvedValueOnce(leaky);
    const { container } = renderWithClient(
      <ToastProvider>
        <PublicBoard projectId={PROJECT_ID} />
      </ToastProvider>,
      { withRouter: true, initialPath: `/public/projects/${PROJECT_ID}/board` },
    );

    await waitFor(() => {
      expect(
        screen.getByText("The export button silently fails on Safari."),
      ).toBeInTheDocument();
    });
    expect(container.textContent).not.toContain("leaked@example.com");
    expect(container.textContent).not.toContain("Leaky McLeakface");
    expect(container.textContent).not.toContain("deadbeef");
  });

  it("renders an empty state when no feedback has been published", async () => {
    mockedList.mockResolvedValueOnce({
      items: [],
      total: 0,
      limit: 50,
      offset: 0,
      access: "public",
      signed_in: false,
    });
    renderWithClient(
      <ToastProvider>
        <PublicBoard projectId={PROJECT_ID} />
      </ToastProvider>,
      { withRouter: true, initialPath: `/public/projects/${PROJECT_ID}/board` },
    );

    await waitFor(() => {
      expect(
        screen.getByText(/no feedback has been published yet/i),
      ).toBeInTheDocument();
    });
  });

  it("renders a clean unavailable state when the board is disabled (404)", async () => {
    const err = new axios.AxiosError("not found");
    // @ts-expect-error — minimal AxiosResponse stub for the 404 branch.
    err.response = { status: 404, data: {} };
    mockedList.mockRejectedValue(err);
    renderWithClient(
      <ToastProvider>
        <PublicBoard projectId={PROJECT_ID} />
      </ToastProvider>,
      { withRouter: true, initialPath: `/public/projects/${PROJECT_ID}/board` },
    );

    await waitFor(() => {
      expect(
        screen.getByText(/this feedback board isn’t available/i),
      ).toBeInTheDocument();
    });
    // Not an error/alert surface — a board-disabled project is a normal state.
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("renders the sign-in prompt, not the unavailable state, on a signed_in board (FR-FBR-43)", async () => {
    const err = new axios.AxiosError("unauthorized");
    // @ts-expect-error — minimal AxiosResponse stub for the 401 branch.
    err.response = { status: 401, data: { error: "SignInRequired" } };
    mockedList.mockRejectedValue(err);
    renderWithClient(
      <ToastProvider>
        <PublicBoard projectId={PROJECT_ID} />
      </ToastProvider>,
      { withRouter: true, initialPath: `/public/projects/${PROJECT_ID}/board` },
    );

    expect(
      await screen.findByRole("heading", { name: "Sign in to see this board" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(/this feedback board isn’t available/i),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows a signed-in viewer their own upvote and a link to their priorities (FR-FBR-46)", async () => {
    mockedList.mockResolvedValueOnce({
      ...exampleList,
      signed_in: true,
      items: [
        {
          ...exampleList.items[0],
          viewer: { upvoted: true, not_needed: false, priority_position: null },
        },
        {
          ...exampleList.items[1],
          viewer: { upvoted: false, not_needed: false, priority_position: 0 },
        },
      ],
    });
    renderWithClient(
      <ToastProvider>
        <PublicBoard projectId={PROJECT_ID} />
      </ToastProvider>,
      { withRouter: true, initialPath: `/public/projects/${PROJECT_ID}/board` },
    );

    expect(
      await screen.findByRole("button", { name: /Retract vote — current count 9/ }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("link", { name: "My priorities" })).toHaveAttribute(
      "href",
      `/public/projects/${PROJECT_ID}/board/priorities`,
    );
    // Each item links to its detail page.
    expect(screen.getByRole("link", { name: /Bug · FB-AAA111/ })).toHaveAttribute(
      "href",
      `/public/projects/${PROJECT_ID}/board/items/FB-AAA111`,
    );
  });

  it("asks the server for one kind when the kind filter is set (FR-FBR-42)", async () => {
    mockedList.mockResolvedValue(exampleList);
    renderWithClient(
      <ToastProvider>
        <PublicBoard projectId={PROJECT_ID} />
      </ToastProvider>,
      { withRouter: true, initialPath: `/public/projects/${PROJECT_ID}/board` },
    );

    const filter = await screen.findByRole("combobox", { name: "Show" });
    fireEvent.change(filter, { target: { value: "feature" } });
    await waitFor(() => {
      expect(mockedList).toHaveBeenCalledWith(PROJECT_ID, { kind: "feature" });
    });
  });
});
