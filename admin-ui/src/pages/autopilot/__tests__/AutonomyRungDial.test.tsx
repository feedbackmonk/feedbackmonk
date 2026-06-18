import { describe, expect, it, vi } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { renderWithClient } from "../../../test/testUtils";
import { AutonomyRungDial } from "../AutonomyRungDial";

describe("AutonomyRungDial", () => {
  it("offers only selectable rungs 1–3 (Rung 0 never produces a work order)", () => {
    renderWithClient(
      <AutonomyRungDial value={1} onChange={() => {}} idPrefix="t" />,
    );
    const radios = screen.getAllByRole("radio");
    expect(radios).toHaveLength(3);
    expect(screen.getByRole("radio", { name: /Rung 1/ })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /Rung 3/ })).toBeInTheDocument();
    // Rung 0 is described for context but is NOT a selectable radio here.
    expect(screen.queryByRole("radio", { name: /Rung 0/ })).toBeNull();
  });

  it("surfaces what each rung AUTHORIZES (blast radius visible at choice time)", () => {
    renderWithClient(
      <AutonomyRungDial value={1} onChange={() => {}} idPrefix="t" />,
    );
    expect(screen.getByText(/drafts a work order for your review/i)).toBeInTheDocument();
    expect(screen.getByText(/Low-stakes actions auto-execute/i)).toBeInTheDocument();
    expect(screen.getByText(/acts on approved orders and reports back/i)).toBeInTheDocument();
  });

  it("reports the chosen rung via onChange", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    renderWithClient(
      <AutonomyRungDial value={1} onChange={onChange} idPrefix="t" />,
    );
    await user.click(screen.getByRole("radio", { name: /Rung 2/ }));
    expect(onChange).toHaveBeenCalledWith(2);
  });
});
