import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import { CrashBanner, safeHttpUrl } from "./CrashBanner";
import { renderWithClient } from "../test/testUtils";

vi.mock("../shared/ApiClient", () => ({ fetchFeedbackCrash: vi.fn() }));

import { fetchFeedbackCrash } from "../shared/ApiClient";
const mockedCrash = vi.mocked(fetchFeedbackCrash);

describe("CrashBanner (parity gap #2)", () => {
  beforeEach(() => {
    mockedCrash.mockReset();
  });

  it("renders a linked crash as plain text with a safe tracker link", async () => {
    mockedCrash.mockResolvedValue({
      status: "linked",
      crash_event_id: "evt-abc",
      crash: {
        crash_event_id: "evt-abc",
        title: "TypeError: cannot read 'id' of undefined",
        culprit: "renderBanner (app/banner.tsx)",
        level: "fatal",
        permalink: "https://glitchtip.example/events/abc/",
        last_seen: "2026-06-02T11:59:00Z",
      },
    });
    renderWithClient(<CrashBanner feedbackId="FB-ABCDEF" crashEventId="evt-stored" />);
    await waitFor(() =>
      expect(screen.getByText("TypeError: cannot read 'id' of undefined")).toBeInTheDocument(),
    );
    expect(screen.getByText("renderBanner (app/banner.tsx)")).toBeInTheDocument();
    const link = screen.getByRole("link", { name: "Open in crash tracker" });
    expect(link).toHaveAttribute("href", "https://glitchtip.example/events/abc/");
    expect(link).toHaveAttribute("rel", "noopener noreferrer");
  });

  it("never links a non-http permalink", async () => {
    mockedCrash.mockResolvedValue({
      status: "linked",
      crash_event_id: "evt-x",
      crash: { crash_event_id: "evt-x", title: "Boom", permalink: "javascript:alert(1)" },
    });
    renderWithClient(<CrashBanner feedbackId="FB-ABCDEF" crashEventId="evt-stored" />);
    await waitFor(() => expect(screen.getByText("Boom")).toBeInTheDocument());
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });

  it("degrades to 'details unavailable' with the id still shown", async () => {
    mockedCrash.mockResolvedValue({ status: "unavailable", crash_event_id: "evt-down" });
    renderWithClient(<CrashBanner feedbackId="FB-ABCDEF" crashEventId="evt-stored" />);
    await waitFor(() =>
      expect(screen.getByText("Crash evt-down — details unavailable.")).toBeInTheDocument(),
    );
  });

  it("says when the tracker has no such event", async () => {
    mockedCrash.mockResolvedValue({ status: "not_found", crash_event_id: "evt-gone" });
    renderWithClient(<CrashBanner feedbackId="FB-ABCDEF" crashEventId="evt-stored" />);
    await waitFor(() =>
      expect(
        screen.getByText("Crash evt-gone — the crash tracker has no such event."),
      ).toBeInTheDocument(),
    );
  });

  it("degrades to 'details unavailable' when the request itself fails", async () => {
    mockedCrash.mockRejectedValue(new Error("network"));
    renderWithClient(<CrashBanner feedbackId="FB-ABCDEF" crashEventId="evt-stored" />);
    await waitFor(() =>
      expect(screen.getByText("Crash evt-stored — details unavailable.")).toBeInTheDocument(),
    );
  });
});

describe("safeHttpUrl", () => {
  it("keeps http(s) and drops everything else", () => {
    expect(safeHttpUrl("https://a.example/x")).toBe("https://a.example/x");
    expect(safeHttpUrl("http://a.example/")).toBe("http://a.example/");
    expect(safeHttpUrl("javascript:alert(1)")).toBeNull();
    expect(safeHttpUrl("data:text/html,x")).toBeNull();
    expect(safeHttpUrl("not a url")).toBeNull();
    expect(safeHttpUrl(null)).toBeNull();
  });
});
