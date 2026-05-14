import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StatusControls } from "./StatusControls";
import { renderWithClient } from "../test/testUtils";

vi.mock("../shared/ApiClient", () => ({
  postTransition: vi.fn(),
}));

import { postTransition } from "../shared/ApiClient";
const mockedTransition = vi.mocked(postTransition);

describe("StatusControls — state-machine rendering invariant", () => {
  beforeEach(() => {
    mockedTransition.mockReset();
  });

  it("from `submitted` offers only triaged / wontfix / duplicate buttons", () => {
    renderWithClient(
      <StatusControls feedbackId="FB-AAAAAA" currentStatus="submitted" />,
    );

    // Allowed targets present.
    expect(screen.getByRole("button", { name: "Triaged" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Won't Fix" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Duplicate" }),
    ).toBeInTheDocument();

    // Disallowed targets absent.
    expect(
      screen.queryByRole("button", { name: "Shipped" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "In Progress" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Submitted" }),
    ).not.toBeInTheDocument();
  });

  it("from `triaged` offers in-progress / wontfix / duplicate / submitted", () => {
    renderWithClient(
      <StatusControls feedbackId="FB-AAAAAA" currentStatus="triaged" />,
    );
    expect(
      screen.getByRole("button", { name: "In Progress" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Won't Fix" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Duplicate" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Submitted" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Shipped" }),
    ).not.toBeInTheDocument();
  });

  it("from `shipped` (terminal) renders no transition buttons", () => {
    renderWithClient(
      <StatusControls feedbackId="FB-AAAAAA" currentStatus="shipped" />,
    );
    expect(
      screen.queryByRole("button", { name: /shipped|triaged|in progress|won/i }),
    ).not.toBeInTheDocument();
    expect(screen.getByText(/Terminal/i)).toBeInTheDocument();
  });

  it("opening a non-duplicate target dialog does not require a target FB id", async () => {
    const user = userEvent.setup();
    renderWithClient(
      <StatusControls feedbackId="FB-AAAAAA" currentStatus="submitted" />,
    );
    await user.click(screen.getByRole("button", { name: "Triaged" }));
    expect(
      screen.getByRole("dialog", { name: /Transition to Triaged/i }),
    ).toBeInTheDocument();
    expect(
      screen.queryByLabelText(/Duplicate of/i),
    ).not.toBeInTheDocument();
  });

  it("opening a `duplicate` target dialog requires a target FB id before submit", async () => {
    const user = userEvent.setup();
    renderWithClient(
      <StatusControls feedbackId="FB-AAAAAA" currentStatus="submitted" />,
    );
    await user.click(screen.getByRole("button", { name: "Duplicate" }));
    const dupInput = screen.getByLabelText(/Duplicate of/i);
    expect(dupInput).toBeRequired();

    // Empty submit -> mutation never fires.
    await user.click(screen.getByRole("button", { name: /Confirm/i }));
    expect(mockedTransition).not.toHaveBeenCalled();
  });

  it("confirming a transition calls postTransition with the chosen target", async () => {
    const user = userEvent.setup();
    mockedTransition.mockResolvedValueOnce({
      feedback_id: "FB-AAAAAA",
      from_status: "submitted",
      to_status: "triaged",
      transitioned_at: "2026-05-14T00:00:00Z",
      audit_id: "00000000-0000-0000-0000-000000000000",
      email_queued: true,
    });

    renderWithClient(
      <StatusControls feedbackId="FB-AAAAAA" currentStatus="submitted" />,
    );

    await user.click(screen.getByRole("button", { name: "Triaged" }));
    await user.click(screen.getByRole("button", { name: /Confirm/i }));

    expect(mockedTransition).toHaveBeenCalledTimes(1);
    expect(mockedTransition).toHaveBeenCalledWith("FB-AAAAAA", {
      to_status: "triaged",
      reason_note: undefined,
      duplicate_of: undefined,
    });
  });
});
