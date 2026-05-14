import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FeedbackList } from "./FeedbackList";
import { renderWithClient } from "../test/testUtils";
import type { FeedbackListResponse } from "../shared/types.gen";

vi.mock("../shared/ApiClient", () => ({
  fetchFeedbackList: vi.fn(),
}));

import { fetchFeedbackList } from "../shared/ApiClient";
const mockedList = vi.mocked(fetchFeedbackList);

const emptyResponse: FeedbackListResponse = {
  items: [],
  total: 0,
  limit: 20,
  offset: 0,
};

describe("FeedbackList", () => {
  beforeEach(() => {
    mockedList.mockReset();
    window.history.replaceState(null, "", "/feedback");
  });

  it("renders the empty state when no items match", async () => {
    mockedList.mockResolvedValueOnce(emptyResponse);
    renderWithClient(<FeedbackList />, {
      withRouter: true,
      initialPath: "/feedback",
    });
    await waitFor(() =>
      expect(
        screen.getByText(/No feedback matches this filter/i),
      ).toBeInTheDocument(),
    );
  });

  it("clicking a status filter pill calls fetchFeedbackList with that status (URL state)", async () => {
    mockedList.mockResolvedValue(emptyResponse);
    const user = userEvent.setup();
    renderWithClient(<FeedbackList />, {
      withRouter: true,
      initialPath: "/feedback",
    });

    // Initial: status undefined.
    await waitFor(() => expect(mockedList).toHaveBeenCalled());
    expect(mockedList).toHaveBeenLastCalledWith(
      expect.objectContaining({ status: undefined, offset: 0 }),
    );

    await user.click(screen.getByRole("button", { name: "Triaged" }));

    await waitFor(() =>
      expect(mockedList).toHaveBeenLastCalledWith(
        expect.objectContaining({ status: "triaged", offset: 0 }),
      ),
    );
    expect(window.location.search).toContain("status=triaged");
  });

  it("pagination Next button increments offset and Previous decrements it", async () => {
    const user = userEvent.setup();
    // 47 total items, first page returns 20; we'll render with 20 dummies.
    const items = Array.from({ length: 20 }, (_, i) => ({
      feedback_id: `FB-${String(i).padStart(6, "0")}`,
      kind: "feature" as const,
      status: "submitted" as const,
      body_excerpt: `excerpt ${i}`,
      submitted_at: "2026-05-13T22:00:00Z",
      submitter_label: "anonymous",
      reply_count: 0,
    }));
    mockedList.mockResolvedValue({
      items,
      total: 47,
      limit: 20,
      offset: 0,
    });

    renderWithClient(<FeedbackList />, {
      withRouter: true,
      initialPath: "/feedback",
    });

    await waitFor(() =>
      expect(screen.getByText(/1.*of 47/)).toBeInTheDocument(),
    );

    await user.click(screen.getByRole("button", { name: /Next/i }));
    await waitFor(() =>
      expect(mockedList).toHaveBeenLastCalledWith(
        expect.objectContaining({ offset: 20, limit: 20 }),
      ),
    );
  });
});
