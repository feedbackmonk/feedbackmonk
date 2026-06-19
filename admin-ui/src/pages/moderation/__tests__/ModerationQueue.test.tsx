import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ModerationQueue } from "../ModerationQueue";
import { ModerationActions } from "../ModerationActions";
import { renderWithClient } from "../../../test/testUtils";

// Mock the owned client module. The state-machine + queue logic under test is
// pure UI; the network seam (C28) is stubbed.
vi.mock("../../../shared/boardModerationApi", async () => {
  const actual = await vi.importActual<
    typeof import("../../../shared/boardModerationApi")
  >("../../../shared/boardModerationApi");
  return {
    ...actual, // keep LEGAL_MODERATION_TRANSITIONS / MODERATION_STATUS_LABELS
    fetchModerationQueue: vi.fn(),
    postModerate: vi.fn(),
  };
});

import {
  fetchModerationQueue,
  postModerate,
} from "../../../shared/boardModerationApi";
const mockedQueue = vi.mocked(fetchModerationQueue);
const mockedModerate = vi.mocked(postModerate);

const queueResponse = {
  items: [
    {
      // A's confirmed C28 queue-row shape (no reply_count / triage status).
      feedback_id: "FB-000123",
      kind: "bug" as const,
      moderation_status: "pending" as const,
      body_excerpt: "Login button is dead on Safari.",
      submitted_at: "2026-06-18T12:00:00Z",
      submitter_label: "anonymous",
    },
  ],
  total: 1,
  limit: 20,
  offset: 0,
};

describe("ModerationQueue", () => {
  beforeEach(() => {
    mockedQueue.mockReset();
    mockedModerate.mockReset();
  });

  it("renders pending rows with body excerpt + approve/reject actions", async () => {
    mockedQueue.mockResolvedValue(queueResponse);
    renderWithClient(<ModerationQueue />, { withRouter: true });

    expect(await screen.findByText("FB-000123")).toBeInTheDocument();
    expect(
      screen.getByText("Login button is dead on Safari."),
    ).toBeInTheDocument();

    // Default queue filter is `pending` ⇒ approve/reject offered, no self-reset.
    const group = screen.getByRole("group", { name: /Moderate FB-000123/ });
    expect(
      within(group).getByRole("button", { name: "Approved" }),
    ).toBeInTheDocument();
    expect(
      within(group).getByRole("button", { name: "Rejected" }),
    ).toBeInTheDocument();
    expect(
      within(group).queryByRole("button", { name: "Pending" }),
    ).not.toBeInTheDocument();
  });

  it("defaults the queue fetch to status=pending", async () => {
    mockedQueue.mockResolvedValue(queueResponse);
    renderWithClient(<ModerationQueue />, { withRouter: true });
    await waitFor(() =>
      expect(mockedQueue).toHaveBeenCalledWith(
        expect.objectContaining({ status: "pending" }),
      ),
    );
  });
});

describe("ModerationActions — moderation state-machine invariant", () => {
  beforeEach(() => mockedModerate.mockReset());

  it("from `pending` offers Approved/Rejected only (no self-transition)", () => {
    renderWithClient(
      <ModerationActions
        feedbackId="FB-AAAAAA"
        currentStatus="pending"
        invalidateKey={["k"]}
      />,
    );
    expect(
      screen.getByRole("button", { name: "Approved" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Rejected" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Pending" }),
    ).not.toBeInTheDocument();
  });

  it("from `approved` offers Rejected/Pending (pull from board), not Approved", () => {
    renderWithClient(
      <ModerationActions
        feedbackId="FB-AAAAAA"
        currentStatus="approved"
        invalidateKey={["k"]}
      />,
    );
    expect(
      screen.getByRole("button", { name: "Rejected" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Pending" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Approved" }),
    ).not.toBeInTheDocument();
  });

  it("confirming approve calls postModerate with to_status=approved", async () => {
    const user = userEvent.setup();
    mockedModerate.mockResolvedValueOnce({
      feedback_id: "FB-AAAAAA",
      from_status: "pending",
      to_status: "approved",
      moderated_at: "2026-06-19T00:00:00Z",
      audit_id: "00000000-0000-0000-0000-000000000000",
    });

    renderWithClient(
      <ModerationActions
        feedbackId="FB-AAAAAA"
        currentStatus="pending"
        invalidateKey={["k"]}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Approved" }));
    expect(
      screen.getByRole("dialog", { name: /Set FB-AAAAAA to Approved/i }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Confirm Approved/i }));

    expect(mockedModerate).toHaveBeenCalledTimes(1);
    expect(mockedModerate).toHaveBeenCalledWith("FB-AAAAAA", {
      to_status: "approved",
      reason_note: undefined,
    });
  });
});
