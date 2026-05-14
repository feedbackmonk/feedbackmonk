import { describe, expect, it, vi } from "vitest";
import { screen } from "@testing-library/react";
import { PromoteButton, slugSuggest } from "../PromoteButton";
import { renderWithClient } from "../../../test/testUtils";

vi.mock("../../../shared/ApiClient", () => ({
  postPromoteFeedback: vi.fn(),
}));

describe("PromoteButton — conditional render", () => {
  it("renders the promote trigger only when kind=feature && status!==duplicate", () => {
    const { rerender } = renderWithClient(
      <PromoteButton
        feedbackId="FB-AAAAAA"
        kind="feature"
        status="submitted"
        bodyPreview="Allow dark mode in settings"
      />,
      { withRouter: true, initialPath: "/feedback/FB-AAAAAA" },
    );
    expect(
      screen.getByRole("button", { name: /promote to roadmap/i }),
    ).toBeInTheDocument();

    // Re-render with bug kind — button hidden entirely (not disabled).
    rerender(
      <PromoteButton
        feedbackId="FB-AAAAAA"
        kind="bug"
        status="submitted"
        bodyPreview="Crashes when I click X"
      />,
    );
    expect(
      screen.queryByRole("button", { name: /promote to roadmap/i }),
    ).not.toBeInTheDocument();

    // Re-render with duplicate status — button hidden entirely.
    rerender(
      <PromoteButton
        feedbackId="FB-AAAAAA"
        kind="feature"
        status="duplicate"
        bodyPreview="Allow dark mode in settings"
      />,
    );
    expect(
      screen.queryByRole("button", { name: /promote to roadmap/i }),
    ).not.toBeInTheDocument();
  });
});

describe("slugSuggest — client-side slug derivation", () => {
  it("lowercases + replaces non-alnum runs with single dashes + trims", () => {
    expect(slugSuggest("Dark Mode")).toBe("dark-mode");
    expect(slugSuggest("  Hello, World!  ")).toBe("hello-world");
    expect(slugSuggest("Allow toggling X in settings")).toBe(
      "allow-toggling-x-in-settings",
    );
  });

  it("truncates to 80 characters", () => {
    const long = "a ".repeat(100);
    expect(slugSuggest(long).length).toBeLessThanOrEqual(80);
  });

  it("handles non-ASCII via aggressive replacement (server is authoritative)", () => {
    // Server re-validates; UI just needs a usable starter slug.
    expect(slugSuggest("café au lait")).toBe("caf-au-lait");
  });
});
