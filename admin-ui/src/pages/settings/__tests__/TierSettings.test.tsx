import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen, within } from "@testing-library/react";
import { TierSettings } from "../TierSettings";
import { UsageMeter } from "../UsageMeter";
import { UpgradePrompt } from "../UpgradePrompt";
import { extractTierCapExceeded } from "../../../shared/ApiClient";
import type {
  Tier,
  TierStatus,
  TierCapExceededBody,
} from "../../../shared/types.gen";
import { renderWithClient } from "../../../test/testUtils";

vi.mock("../../../shared/ApiClient", async () => {
  const actual = await vi.importActual<
    typeof import("../../../shared/ApiClient")
  >("../../../shared/ApiClient");
  return {
    ...actual,
    fetchTierStatus: vi.fn(),
  };
});

import { fetchTierStatus } from "../../../shared/ApiClient";
const mockedFetchTierStatus = vi.mocked(fetchTierStatus);

// Contract C19 quotas mirror — kept inline so a Stage 1 contract change
// FAILS this test (the canonical drift surface for Stage 2). If Stage 1
// rebases its tier_quotas() shape, both this fixture AND the backend
// `tier-enforcement-status --full` Probe B must update together.
function tierStatus(tier: Tier, projects: number, monthlyFeedback: number): TierStatus {
  const quotas = {
    free: {
      projects_per_org: 1,
      monthly_feedback_volume: 50,
      custom_branding: false,
      custom_domain: false,
      eu_residency: false,
      footer_text: "powered by feedbackmonk",
    },
    starter: {
      projects_per_org: 3,
      monthly_feedback_volume: 500,
      custom_branding: true,
      custom_domain: false,
      eu_residency: false,
      footer_text: null,
    },
    pro: {
      projects_per_org: null,
      monthly_feedback_volume: 10000,
      custom_branding: true,
      custom_domain: true,
      eu_residency: true,
      footer_text: null,
    },
    self_host: {
      projects_per_org: null,
      monthly_feedback_volume: null,
      custom_branding: true,
      custom_domain: true,
      eu_residency: true,
      footer_text: null,
    },
  }[tier] satisfies TierStatus["quotas"];

  return {
    tier,
    quotas,
    usage: {
      projects,
      feedback_monthly: monthlyFeedback,
      period_start: "2026-04-14T00:00:00Z",
    },
  };
}

describe("TierSettings — renders tier status from Contract C17", () => {
  beforeEach(() => {
    mockedFetchTierStatus.mockReset();
  });

  it("Free tier: shows Free badge, projects 0/1 + monthly 0/50, footer capability ON", async () => {
    mockedFetchTierStatus.mockResolvedValueOnce(tierStatus("free", 0, 0));
    renderWithClient(<TierSettings />);

    // The "Plan & usage" heading is static (renders before data loads), so
    // wait on the data-bound tier badge instead.
    expect(await screen.findByText("Free")).toBeInTheDocument();

    const projectsBar = screen.getByRole("progressbar", { name: /Projects/ });
    expect(projectsBar).toHaveAttribute("aria-valuenow", "0");
    expect(projectsBar).toHaveAttribute("aria-valuemax", "100");
    expect(projectsBar).toHaveAccessibleName("Projects");

    const feedbackBar = screen.getByRole("progressbar", {
      name: /Monthly feedback/,
    });
    expect(feedbackBar).toHaveAttribute("aria-valuenow", "0");

    // Free tier: Free-tier footer constraint is active → "no free-tier
    // footer" capability is OFF (rendered as ✗).
    const footerCap = screen
      .getByText(/Free-tier footer/i)
      .closest("li")!;
    expect(footerCap).toHaveClass("capability-off");
  });

  it("Starter tier: projects 2/3 → meter is in warn band; upgrade prompt visible", async () => {
    mockedFetchTierStatus.mockResolvedValueOnce(tierStatus("starter", 2, 100));
    renderWithClient(<TierSettings />);

    expect(await screen.findByText("Starter")).toBeInTheDocument();

    const projectsBar = screen.getByRole("progressbar", { name: /Projects/ });
    // 2/3 ≈ 66% → still in OK band (< 70%).
    expect(projectsBar).toHaveAttribute("aria-valuenow", "67");
    const projectsMeter = projectsBar.closest(".usage-meter")!;
    expect(projectsMeter).toHaveClass("usage-meter-ok");

    // Upgrade CTA renders for Starter.
    expect(
      screen.getByRole("link", { name: /Contact support to upgrade/i }),
    ).toBeInTheDocument();
  });

  it("Pro tier: projects show 'unlimited'; upgrade prompt still visible (Self-host)", async () => {
    mockedFetchTierStatus.mockResolvedValueOnce(tierStatus("pro", 12, 4500));
    renderWithClient(<TierSettings />);

    expect(await screen.findByText("Pro")).toBeInTheDocument();

    // Pro projects are unlimited → no progressbar for projects (just the
    // unlimited row); monthly feedback IS bounded so it has a progressbar.
    const projectsRow = screen
      .getByText("Projects")
      .closest(".usage-meter") as HTMLElement;
    expect(projectsRow).toHaveClass("usage-meter-unlimited");
    expect(within(projectsRow).getByText(/12 \/ unlimited/)).toBeInTheDocument();
    // Monthly feedback bar still present (10000 cap).
    expect(
      screen.getByRole("progressbar", { name: /Monthly feedback/ }),
    ).toHaveAttribute("aria-valuenow", "45");

    expect(
      screen.getByRole("link", { name: /Contact support to upgrade/i }),
    ).toBeInTheDocument();
  });

  it("Self-host tier: BOTH meters unlimited; NO upgrade prompt rendered", async () => {
    mockedFetchTierStatus.mockResolvedValueOnce(
      tierStatus("self_host", 5, 50000),
    );
    renderWithClient(<TierSettings />);

    expect(await screen.findByText("Self-host")).toBeInTheDocument();
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("link", { name: /Contact support to upgrade/i }),
    ).not.toBeInTheDocument();
  });
});

describe("UsageMeter — color-state thresholds", () => {
  it("ratio <70% → ok state", () => {
    const { container } = renderWithClient(
      <UsageMeter label="Projects" current={5} limit={10} />,
    );
    expect(container.querySelector(".usage-meter")).toHaveClass(
      "usage-meter-ok",
    );
    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuenow",
      "50",
    );
  });

  it("ratio 70–95% → warn state", () => {
    const { container } = renderWithClient(
      <UsageMeter label="Monthly feedback" current={40} limit={50} />,
    );
    expect(container.querySelector(".usage-meter")).toHaveClass(
      "usage-meter-warn",
    );
    // aria-valuetext exposes the verbose state for screen readers.
    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuetext",
      expect.stringContaining("approaching cap"),
    );
  });

  it("ratio >95% → danger state", () => {
    const { container } = renderWithClient(
      <UsageMeter label="Monthly feedback" current={49} limit={50} />,
    );
    expect(container.querySelector(".usage-meter")).toHaveClass(
      "usage-meter-danger",
    );
    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuetext",
      expect.stringContaining("over cap"),
    );
  });

  it("limit=null (unlimited) → renders unlimited row, no progressbar", () => {
    renderWithClient(
      <UsageMeter label="Projects" current={42} limit={null} />,
    );
    expect(screen.getByText("42 / unlimited")).toBeInTheDocument();
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
  });
});

describe("UpgradePrompt — render rules", () => {
  it("Free tier: renders the support-mailto button", () => {
    renderWithClient(<UpgradePrompt currentTier="free" />);
    const link = screen.getByRole("link", {
      name: /Contact support to upgrade/i,
    });
    expect(link).toHaveAttribute(
      "href",
      expect.stringMatching(/^mailto:support@feedbackmonk\.com/),
    );
  });

  it("Self-host tier: renders nothing (no upsell from cap-free tier)", () => {
    const { container } = renderWithClient(
      <UpgradePrompt currentTier="self_host" />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("Custom message overrides default upsell copy", () => {
    renderWithClient(
      <UpgradePrompt currentTier="free" message="Custom upsell text." />,
    );
    expect(screen.getByText("Custom upsell text.")).toBeInTheDocument();
  });
});

describe("extractTierCapExceeded — Contract C18 detection", () => {
  it("returns the parsed body when AxiosError carries tier_cap_exceeded shape", () => {
    const body: TierCapExceededBody = {
      error: "tier_cap_exceeded",
      tier: "free",
      resource: "feedback_in_rolling_month",
      current: 50,
      limit: 50,
      upgrade_hint: "Upgrade to Starter for 500 monthly feedback.",
    };
    const fakeAxiosError = {
      isAxiosError: true,
      response: { status: 402, data: body },
    };
    expect(extractTierCapExceeded(fakeAxiosError)).toEqual(body);
  });

  it("returns null for unrelated errors", () => {
    expect(extractTierCapExceeded(new Error("boom"))).toBeNull();
    expect(extractTierCapExceeded(null)).toBeNull();
    expect(extractTierCapExceeded({ response: { status: 500 } })).toBeNull();
  });
});
