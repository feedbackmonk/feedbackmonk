import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ReplyComposer, REPLY_MAX } from "./ReplyComposer";
import { renderWithClient } from "../test/testUtils";

vi.mock("../shared/ApiClient", () => ({
  postReply: vi.fn(),
}));

import { postReply } from "../shared/ApiClient";
const mockedReply = vi.mocked(postReply);

describe("ReplyComposer", () => {
  beforeEach(() => {
    mockedReply.mockReset();
  });

  it("rejects empty body (0 chars) — submit disabled and postReply not called", async () => {
    const user = userEvent.setup();
    renderWithClient(<ReplyComposer feedbackId="FB-AAAAAA" />);
    const submit = screen.getByRole("button", { name: /Send reply/i });
    expect(submit).toBeDisabled();
    await user.click(submit);
    expect(mockedReply).not.toHaveBeenCalled();
  });

  it("rejects over-limit body — submit disabled past REPLY_MAX", async () => {
    const user = userEvent.setup();
    renderWithClient(<ReplyComposer feedbackId="FB-AAAAAA" />);
    const textarea = screen.getByLabelText(/Reply body/i);
    // Paste an over-limit string in one shot rather than typing — much faster.
    await user.click(textarea);
    await user.paste("x".repeat(REPLY_MAX + 1));
    const submit = screen.getByRole("button", { name: /Send reply/i });
    expect(submit).toBeDisabled();
    expect(mockedReply).not.toHaveBeenCalled();
  });

  it("toggles between Public and Internal visibility and adjusts submit label", async () => {
    const user = userEvent.setup();
    renderWithClient(<ReplyComposer feedbackId="FB-AAAAAA" />);

    expect(
      screen.getByRole("button", { name: /Send reply/i }),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("radio", { name: /Internal/i }),
    );
    expect(
      screen.getByRole("button", { name: /Save internal note/i }),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("radio", { name: /Public/i }));
    expect(
      screen.getByRole("button", { name: /Send reply/i }),
    ).toBeInTheDocument();
  });

  it("submits a valid reply exactly once with the entered body + visibility", async () => {
    const user = userEvent.setup();
    mockedReply.mockResolvedValueOnce({
      reply_id: "00000000-0000-0000-0000-000000000001",
      feedback_id: "FB-AAAAAA",
      visibility: "public",
      created_at: "2026-05-14T00:00:00Z",
      email_queued: true,
    });

    renderWithClient(<ReplyComposer feedbackId="FB-AAAAAA" />);
    await user.type(screen.getByLabelText(/Reply body/i), "Hello there.");
    await user.click(screen.getByRole("button", { name: /Send reply/i }));

    expect(mockedReply).toHaveBeenCalledTimes(1);
    expect(mockedReply).toHaveBeenCalledWith("FB-AAAAAA", {
      body: "Hello there.",
      visibility: "public",
    });
  });
});
