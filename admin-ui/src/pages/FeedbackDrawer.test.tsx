import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FeedbackDrawer } from "./FeedbackDrawer";
import { renderWithClient } from "../test/testUtils";
import type { FeedbackDetail } from "../shared/types.gen";

// Mock the API client (drawer only calls fetchFeedbackDetail) + the child
// components that fetch/mutate, so the test isolates the drawer's body/toggle.
vi.mock("../shared/ApiClient", () => ({ fetchFeedbackDetail: vi.fn() }));
vi.mock("../components/StatusControls", () => ({ StatusControls: () => null }));
vi.mock("../components/ReplyComposer", () => ({ ReplyComposer: () => null }));
vi.mock("./roadmap/PromoteButton", () => ({ PromoteButton: () => null }));

import { fetchFeedbackDetail } from "../shared/ApiClient";
const mockedDetail = vi.mocked(fetchFeedbackDetail);

const baseDetail: FeedbackDetail = {
  feedback_id: "FB-ABCDEF",
  kind: "bug",
  status: "submitted",
  body: "Der Login-Button reagiert nicht.",
  submitted_at: "2026-06-21T22:00:00Z",
  submitter: { kind: "anonymous" },
  status_history: [],
  replies: [],
  sentiment: null,
};

describe("FeedbackDrawer translation toggle (FR-FBR-30 #3)", () => {
  beforeEach(() => {
    mockedDetail.mockReset();
  });

  it("shows the original by default and toggles to the translation and back", async () => {
    mockedDetail.mockResolvedValue({
      ...baseDetail,
      body_translated: "The login button does not respond.",
      source_lang: "DE",
      translation_status: "translated",
    });
    const user = userEvent.setup();
    renderWithClient(<FeedbackDrawer feedbackId="FB-ABCDEF" onClose={() => {}} />);

    // Original shown first.
    await waitFor(() =>
      expect(
        screen.getByText("Der Login-Button reagiert nicht."),
      ).toBeInTheDocument(),
    );
    expect(
      screen.queryByText("The login button does not respond."),
    ).not.toBeInTheDocument();

    // Toggle on → translation shown, original hidden.
    const toggle = screen.getByRole("button", { name: /Show translation \(from DE\)/i });
    await user.click(toggle);
    expect(
      screen.getByText("The login button does not respond."),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("Der Login-Button reagiert nicht."),
    ).not.toBeInTheDocument();

    // Toggle off → back to the original.
    await user.click(screen.getByRole("button", { name: /Show original/i }));
    expect(
      screen.getByText("Der Login-Button reagiert nicht."),
    ).toBeInTheDocument();
  });

  it("renders no toggle when the row is untranslated", async () => {
    mockedDetail.mockResolvedValue({
      ...baseDetail,
      body: "Plain English feedback.",
      body_translated: null,
      source_lang: null,
      translation_status: null,
    });
    renderWithClient(<FeedbackDrawer feedbackId="FB-ABCDEF" onClose={() => {}} />);

    await waitFor(() =>
      expect(screen.getByText("Plain English feedback.")).toBeInTheDocument(),
    );
    expect(
      screen.queryByRole("button", { name: /Show translation/i }),
    ).not.toBeInTheDocument();
  });
});
