import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { extractSearchTerms, highlightMatches } from "./highlight";

describe("extractSearchTerms", () => {
  it("keeps quoted phrases whole and drops excluded terms and operators", () => {
    expect(extractSearchTerms('"login button" safari -android or chrome')).toEqual(
      ["login button", "safari", "chrome"],
    );
  });

  it("drops stopwords and single characters", () => {
    expect(extractSearchTerms("the a x crash")).toEqual(["crash"]);
  });

  it("lightly stems inflected words so a differently-inflected hit still lights up", () => {
    expect(extractSearchTerms("buttons crashed loading categories")).toEqual([
      "category",
      "button",
      "crash",
      "load",
    ]);
  });

  it("orders longest-first and de-duplicates", () => {
    expect(extractSearchTerms("check checkout Check")).toEqual([
      "checkout",
      "check",
    ]);
  });

  it("returns nothing for a blank query", () => {
    expect(extractSearchTerms("   ")).toEqual([]);
  });
});

describe("highlightMatches", () => {
  function marks(text: string, query: string): string[] {
    const { container } = render(<p>{highlightMatches(text, query)}</p>);
    return Array.from(container.querySelectorAll("mark")).map(
      (m) => m.textContent ?? "",
    );
  }

  it("wraps case-insensitive occurrences in <mark> and preserves the rest of the text", () => {
    const { container } = render(
      <p>{highlightMatches("Login button on mobile Safari.", "safari button")}</p>,
    );
    expect(container.textContent).toBe("Login button on mobile Safari.");
    expect(
      Array.from(container.querySelectorAll("mark")).map((m) => m.textContent),
    ).toEqual(["button", "Safari"]);
  });

  it("prefers the longer term when one is a prefix of another", () => {
    expect(marks("The checkout page", "check checkout")).toEqual(["checkout"]);
  });

  it("returns the plain string when nothing matches or the query has no terms", () => {
    expect(highlightMatches("No hits here", "zebra")).toBe("No hits here");
    expect(highlightMatches("No hits here", "-excluded or")).toBe("No hits here");
  });

  it("treats regex metacharacters in the query literally", () => {
    expect(marks("Costs $4.99 (approx)", '"$4.99"')).toEqual(["$4.99"]);
  });
});
