import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import { SearchBox } from "./SearchBox";

// Debounce tests drive the input with synchronous `fireEvent.change` + manual
// timer advancement. (userEvent's internal delays deadlock against fake timers,
// so it is intentionally not used here.)
describe("SearchBox", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
  });

  function box() {
    return screen.getByRole("searchbox", {
      name: /search feedback/i,
    }) as HTMLInputElement;
  }

  it("is accessible: search landmark + labelled searchbox", () => {
    render(<SearchBox value="" onSearch={vi.fn()} />);
    // role=search landmark wraps the field.
    expect(screen.getByRole("search")).toBeInTheDocument();
    // type=search → role searchbox; accessible name comes from the <label>,
    // so this query passing IS the label-association assertion.
    expect(
      screen.getByRole("searchbox", { name: /search feedback/i }),
    ).toBeInTheDocument();
  });

  it("debounces: fires onSearch once after the delay, with the trimmed query", () => {
    const onSearch = vi.fn();
    render(<SearchBox value="" onSearch={onSearch} delayMs={250} />);

    // Each keystroke restarts the timer; only the last settles.
    fireEvent.change(box(), { target: { value: "che" } });
    fireEvent.change(box(), { target: { value: "  checkout  " } });
    expect(onSearch).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(250);
    });

    expect(onSearch).toHaveBeenCalledTimes(1);
    expect(onSearch).toHaveBeenCalledWith("checkout");
  });

  it("does not fire when the trimmed text equals the committed value", () => {
    const onSearch = vi.fn();
    render(<SearchBox value="bug" onSearch={onSearch} delayMs={250} />);

    // Type past "bug" then back to it — net no change vs the committed value.
    fireEvent.change(box(), { target: { value: "bugs" } });
    fireEvent.change(box(), { target: { value: "bug" } });

    act(() => {
      vi.advanceTimersByTime(250);
    });
    expect(onSearch).not.toHaveBeenCalled();
  });

  it("clear button resets the field and emits an empty query", () => {
    const onSearch = vi.fn();
    render(<SearchBox value="theme" onSearch={onSearch} delayMs={250} />);

    fireEvent.click(screen.getByRole("button", { name: /clear search/i }));

    expect(onSearch).toHaveBeenLastCalledWith("");
    expect(box().value).toBe("");
  });

  it("Escape clears the field and emits an empty query", () => {
    const onSearch = vi.fn();
    render(<SearchBox value="theme" onSearch={onSearch} delayMs={250} />);

    fireEvent.keyDown(box(), { key: "Escape" });

    expect(onSearch).toHaveBeenLastCalledWith("");
    expect(box().value).toBe("");
  });

  it("does not render Clear when the field is empty", () => {
    render(<SearchBox value="" onSearch={vi.fn()} />);
    expect(
      screen.queryByRole("button", { name: /clear search/i }),
    ).not.toBeInTheDocument();
  });

  it("pressing / outside an editable control focuses the field; inside one it does not", () => {
    render(
      <div>
        <SearchBox value="" onSearch={vi.fn()} />
        <textarea aria-label="Reply" />
      </div>,
    );
    const reply = screen.getByRole("textbox", { name: "Reply" });

    // From the page body → focuses search.
    fireEvent.keyDown(document.body, { key: "/" });
    expect(box()).toHaveFocus();

    // From another editable control → leaves focus alone (the "/" is typing).
    reply.focus();
    fireEvent.keyDown(reply, { key: "/" });
    expect(reply).toHaveFocus();

    // Modified keypress is not the shortcut.
    reply.blur();
    fireEvent.keyDown(document.body, { key: "/", ctrlKey: true });
    expect(box()).not.toHaveFocus();
  });

  it("focusKey={null} disables the shortcut and hides the hint", () => {
    render(<SearchBox value="" onSearch={vi.fn()} focusKey={null} />);
    fireEvent.keyDown(document.body, { key: "/" });
    expect(box()).not.toHaveFocus();
    expect(box()).not.toHaveAttribute("aria-keyshortcuts");
  });

  it("describes the field with the syntax hint", () => {
    render(<SearchBox value="" onSearch={vi.fn()} />);
    expect(box()).toHaveAccessibleDescription(/exact phrase/i);
  });

  it("syncs the input when the committed value changes externally", () => {
    const { rerender } = render(<SearchBox value="" onSearch={vi.fn()} />);
    rerender(<SearchBox value="external" onSearch={vi.fn()} />);
    expect(box().value).toBe("external");
  });
});
