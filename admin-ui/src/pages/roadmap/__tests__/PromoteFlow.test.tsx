import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PromoteButton } from "../PromoteButton";
import { ToastProvider } from "../../../components/Toast";
import { renderWithClient } from "../../../test/testUtils";
import type { PromoteResponse } from "../../../shared/types.gen";

vi.mock("../../../shared/ApiClient", () => ({
  postPromoteFeedback: vi.fn(),
}));

import { postPromoteFeedback } from "../../../shared/ApiClient";
const mockedPromote = vi.mocked(postPromoteFeedback);

const happyResponse: PromoteResponse = {
  roadmap_item_id: "00000000-0000-0000-0000-000000000001",
  roadmap_item_slug: "allow-dark-mode",
  source_feedback_id: "FB-AAAAAA",
  source_status: "duplicate",
  already_promoted: false,
};

const idempotentResponse: PromoteResponse = {
  ...happyResponse,
  already_promoted: true,
};

function renderHarness() {
  return renderWithClient(
    <ToastProvider>
      <PromoteButton
        feedbackId="FB-AAAAAA"
        kind="feature"
        status="submitted"
        bodyPreview="Allow dark mode in settings"
      />
    </ToastProvider>,
    { withRouter: true, initialPath: "/feedback/FB-AAAAAA" },
  );
}

describe("PromoteFlow", () => {
  beforeEach(() => {
    mockedPromote.mockReset();
    window.history.replaceState(null, "", "/feedback/FB-AAAAAA");
  });

  it("happy path: 200 already_promoted=false → success toast + navigate to /admin/roadmap with highlight", async () => {
    mockedPromote.mockResolvedValueOnce(happyResponse);
    const user = userEvent.setup();
    renderHarness();

    await user.click(
      screen.getByRole("button", { name: /promote to roadmap/i }),
    );
    // Slug input is pre-populated via slugSuggest(body).
    const submit = screen.getByRole("button", { name: /^promote$/i });
    await user.click(submit);

    await waitFor(() => {
      expect(mockedPromote).toHaveBeenCalledWith(
        "FB-AAAAAA",
        expect.objectContaining({ slug: expect.any(String) }),
      );
    });

    await waitFor(() => {
      expect(window.location.pathname).toBe("/admin/roadmap");
    });
    expect(window.location.search).toContain("highlight=allow-dark-mode");
    expect(screen.getByText(/promoted to roadmap/i)).toBeInTheDocument();
  });

  it("idempotent path: 200 already_promoted=true → info toast", async () => {
    mockedPromote.mockResolvedValueOnce(idempotentResponse);
    const user = userEvent.setup();
    renderHarness();

    await user.click(
      screen.getByRole("button", { name: /promote to roadmap/i }),
    );
    await user.click(screen.getByRole("button", { name: /^promote$/i }));

    await waitFor(() => expect(mockedPromote).toHaveBeenCalled());
    await waitFor(() => {
      expect(
        screen.getByText(/already promoted to the roadmap/i),
      ).toBeInTheDocument();
    });
  });

  it("400 InvalidSlug → inline error renders inside dialog", async () => {
    const axiosError = {
      isAxiosError: true,
      response: {
        status: 400,
        data: { error: "InvalidSlug", slug: "BAD_SLUG" },
      },
    };
    mockedPromote.mockRejectedValueOnce(axiosError);
    const user = userEvent.setup();
    renderHarness();

    await user.click(
      screen.getByRole("button", { name: /promote to roadmap/i }),
    );
    await user.click(screen.getByRole("button", { name: /^promote$/i }));

    await waitFor(() => {
      expect(screen.getByRole("alert")).toHaveTextContent(
        /kebab-case|slug must be/i,
      );
    });
  });
});
