import { describe, expect, it, vi, beforeEach } from "vitest";
import axios from "axios";
import { screen, waitFor } from "@testing-library/react";
import { PublicBoard } from "../PublicBoard";
import { ToastProvider } from "../../../components/Toast";
import { renderWithClient } from "../../../test/testUtils";
import type { BoardListResponse } from "../../../shared/types.gen";

vi.mock("../../../shared/ApiClient", () => ({
  fetchPublicBoard: vi.fn(),
}));

import { fetchPublicBoard } from "../../../shared/ApiClient";
const mockedList = vi.mocked(fetchPublicBoard);

const PROJECT_ID = "00000000-0000-0000-0000-000000000abc";

const exampleList: BoardListResponse = {
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
};

describe("PublicBoard", () => {
  beforeEach(() => {
    mockedList.mockReset();
  });

  it("renders approved board items with body, kind, status and read-only vote count", async () => {
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
    // Vote count is read-only this stage (voting deferred — no vote button).
    expect(screen.getByText("14 votes")).toBeInTheDocument();
    expect(screen.getByText("9 votes")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /vote/i })).not.toBeInTheDocument();
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
    } as BoardListResponse;
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
});
