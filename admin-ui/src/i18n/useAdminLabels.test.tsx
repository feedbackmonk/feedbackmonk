import { afterEach, describe, expect, it } from "vitest";
import { renderHook } from "@testing-library/react";
import { useAdminLabels } from "./useAdminLabels";
import { i18n } from "./index";

afterEach(async () => {
  await i18n.changeLanguage("en");
  i18n.removeResourceBundle("de", "admin");
});

describe("useAdminLabels", () => {
  it("renders the English catalog values by default", () => {
    const { result } = renderHook(() => useAdminLabels());
    expect(result.current.tier("free")).toBe("Free");
    expect(result.current.workOrderState("draft")).toBe("Draft");
    expect(result.current.actionType("bug_fix")).toBe("Bug fix");
    expect(result.current.clusterPriority("high")).toBe("High");
    expect(result.current.clusterStatus("open")).toBe("Open");
    expect(result.current.recommendationStatus("proposed")).toBe("Proposed");
    expect(result.current.autonomyRungLabel(1)).toContain("Draft");
    expect(result.current.workOrderEvent("approve")).toBe("Approved");
    expect(result.current.keyClass("runner")).toBe("Runner (autonomous agent)");
    expect(result.current.moderationStatus("pending")).toBe("Pending");
    expect(result.current.tokenLifecycle("active")).toBe("Active");
  });

  it("uses the active locale's value when the catalog has one", async () => {
    i18n.addResourceBundle("de", "admin", {
      admin: { enum: { tier: { free: "Kostenlos" } } },
    });
    await i18n.changeLanguage("de");
    const { result } = renderHook(() => useAdminLabels());
    expect(result.current.tier("free")).toBe("Kostenlos");
  });

  it("falls back per key to English — a partial catalog is shippable (C35 rule 6)", async () => {
    i18n.addResourceBundle("de", "admin", {
      admin: { enum: { tier: { free: "Kostenlos" } } },
    });
    await i18n.changeLanguage("de");
    const { result } = renderHook(() => useAdminLabels());
    expect(result.current.tier("free")).toBe("Kostenlos");
    expect(result.current.tier("pro")).toBe("Pro");
  });

  it("falls back to the wire value for an enum value no catalog knows", () => {
    const { result } = renderHook(() => useAdminLabels());
    expect(result.current.clusterStatus("unknown-status" as never)).toBe(
      "unknown-status",
    );
  });
});
