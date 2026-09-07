import { afterEach, describe, expect, it } from "vitest";
import { renderHook } from "@testing-library/react";
import { useLabels } from "./useLabels";
import { i18n } from "./index";
import type { FeedbackStatus } from "../shared/types.gen";

afterEach(async () => {
  await i18n.changeLanguage("en");
  i18n.removeResourceBundle("de", "status");
});

describe("useLabels", () => {
  it("renders the English catalog values by default", () => {
    const { result } = renderHook(() => useLabels());
    expect(result.current.status("wontfix")).toBe("Won't Fix");
    expect(result.current.status("in-progress")).toBe("In Progress");
    expect(result.current.kind("bug")).toBe("Bug");
    expect(result.current.sentiment("positive")).toBe("Positive");
    // The roadmap wording differs from the feedback wording, deliberately.
    expect(result.current.roadmapStatus("wontfix")).toBe("Won't Do");
  });

  it("uses the active locale's value when the catalog has one", async () => {
    i18n.addResourceBundle("de", "status", {
      status: { wontfix: "Wird nicht behoben" },
    });
    await i18n.changeLanguage("de");
    const { result } = renderHook(() => useLabels());
    expect(result.current.status("wontfix")).toBe("Wird nicht behoben");
  });

  it("falls back per key to English — a partial catalog is shippable (C35 rule 6)", async () => {
    i18n.addResourceBundle("de", "status", {
      status: { wontfix: "Wird nicht behoben" },
    });
    await i18n.changeLanguage("de");
    const { result } = renderHook(() => useLabels());
    // Translated in `de`…
    expect(result.current.status("wontfix")).toBe("Wird nicht behoben");
    // …everything else still renders, in English, never as a raw key.
    expect(result.current.status("triaged")).toBe("Triaged");
    expect(result.current.kind("feature")).toBe("Feature");
    expect(result.current.roadmapStatus("planned")).toBe("Planned");
  });

  it("falls back to the wire value for an enum value no catalog knows", () => {
    const { result } = renderHook(() => useLabels());
    // A value the server could add before the UI knows about it: it must
    // render as itself, never as `status.escalated`.
    expect(result.current.status("escalated" as FeedbackStatus)).toBe(
      "escalated",
    );
  });
});
