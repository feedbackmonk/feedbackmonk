import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen, within, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { OwnerPriority } from "../OwnerPriority";
import { uncertaintyBand } from "../uncertainty";
import type {
  BoardNoteResponse,
  PriorityView,
} from "../../../shared/boardModerationApi";
import { renderWithClient } from "../../../test/testUtils";

// The owner's priority view (FR-FBR-55/56, Contract C44) and the board-note
// editor (FR-FBR-58), both driven through the mocked admin client.

vi.mock("../../../shared/ApiClient", async () => {
  const actual = await vi.importActual<
    typeof import("../../../shared/ApiClient")
  >("../../../shared/ApiClient");
  return { ...actual, fetchAdminProjects: vi.fn() };
});

vi.mock("../../../shared/boardModerationApi", async () => {
  const actual = await vi.importActual<
    typeof import("../../../shared/boardModerationApi")
  >("../../../shared/boardModerationApi");
  return {
    ...actual,
    fetchPriorityView: vi.fn(),
    fetchBoardNote: vi.fn(),
    putBoardNote: vi.fn(),
    deleteBoardNote: vi.fn(),
  };
});

import { fetchAdminProjects } from "../../../shared/ApiClient";
import {
  deleteBoardNote,
  fetchBoardNote,
  fetchPriorityView,
  putBoardNote,
} from "../../../shared/boardModerationApi";

const mockedProjects = vi.mocked(fetchAdminProjects);
const mockedView = vi.mocked(fetchPriorityView);
const mockedGetNote = vi.mocked(fetchBoardNote);
const mockedPutNote = vi.mocked(putBoardNote);
const mockedDeleteNote = vi.mocked(deleteBoardNote);

const PROJECT_ID = "11111111-1111-1111-1111-111111111111";

function view(overrides: Partial<PriorityView> = {}): PriorityView {
  return {
    computed_at: "2026-10-13T12:00:00Z",
    contributing_rankers: 14,
    ranked: [
      {
        short_code: "FB-AAAAAA",
        body_excerpt: "Dark mode for the dashboard",
        status: "triaged",
        rank: 1,
        rankers: 9,
        uncertainty: 0.12,
        vote_count: 40,
        not_needed_count: 3,
      },
      {
        short_code: "FB-BBBBBB",
        body_excerpt: "Export to CSV",
        status: "in-progress",
        rank: 2,
        rankers: 5,
        uncertainty: 0.71,
        vote_count: 7,
        not_needed_count: 11,
      },
    ],
    unranked: [
      {
        short_code: "FB-CCCCCC",
        body_excerpt: "Keyboard shortcuts",
        status: "submitted",
        rankers: 0,
        vote_count: 2,
        not_needed_count: 0,
      },
    ],
    ...overrides,
  };
}

function note(body: string | null): BoardNoteResponse {
  return {
    short_code: "FB-AAAAAA",
    note: body === null ? null : { body, updated_at: "2026-10-12T09:00:00Z" },
  };
}

function headers(table: HTMLElement): string[] {
  return within(table)
    .getAllByRole("columnheader")
    .map((h) => h.textContent ?? "");
}

beforeEach(() => {
  for (const m of [
    mockedProjects,
    mockedView,
    mockedGetNote,
    mockedPutNote,
    mockedDeleteNote,
  ]) {
    m.mockReset();
  }
  mockedProjects.mockResolvedValue({
    projects: [
      {
        project_id: PROJECT_ID,
        name: "Acme",
        slug: "acme",
        created_at: "2026-01-01T00:00:00Z",
      },
    ],
  } as Awaited<ReturnType<typeof fetchAdminProjects>>);
});

describe("OwnerPriority — priority beside votes, never merged (FR-FBR-55)", () => {
  it("shows priority, votes and not-needed as separate columns, with no combined score", async () => {
    mockedView.mockResolvedValueOnce(view());
    renderWithClient(<OwnerPriority />);

    const ranked = await screen.findByRole("table", { name: /priority order/i });
    expect(mockedView).toHaveBeenCalledWith(PROJECT_ID);
    const cols = headers(ranked);
    for (const name of [
      "Priority",
      "Ranked by",
      "Uncertainty",
      "Votes",
      "Not needed",
    ]) {
      expect(cols).toContain(name);
    }
    // No column, anywhere on the page, merges the signals into one number.
    for (const table of screen.getAllByRole("table")) {
      for (const h of headers(table)) {
        expect(h).not.toMatch(/score|combined|total|weighted/i);
      }
    }

    // Each signal lands in its own cell of the item's row.
    const row = within(ranked).getByText("FB-BBBBBB").closest("tr")!;
    const cells = within(row as HTMLElement)
      .getAllByRole("cell")
      .map((c) => c.textContent ?? "");
    // The rank is the row header (the first column), so each data cell sits
    // one place before its column header's index.
    expect(cols[0]).toBe("Priority");
    const rankHeader = within(row as HTMLElement).getByRole("rowheader");
    expect(rankHeader).toHaveTextContent("2");
    const cellFor = (name: string) => cells[cols.indexOf(name) - 1];
    expect(cellFor("Ranked by")).toBe("5");
    expect(cellFor("Votes")).toBe("7");
    expect(cellFor("Not needed")).toBe("11");
    expect(cellFor("Uncertainty")).toMatch(/High/);
  });

  it("lists unranked items in their own table, apart from the order", async () => {
    mockedView.mockResolvedValueOnce(view());
    renderWithClient(<OwnerPriority />);

    const ranked = await screen.findByRole("table", { name: /priority order/i });
    const unranked = screen.getByRole("table", { name: /no user has ranked/i });
    expect(within(ranked).queryByText("FB-CCCCCC")).toBeNull();
    expect(within(unranked).getByText("FB-CCCCCC")).toBeInTheDocument();
    expect(headers(unranked)).not.toContain("Priority");
    expect(headers(unranked)).toContain("Votes");
    expect(headers(unranked)).toContain("Not needed");
  });

  it("says when the aggregate was computed and how many lists contribute", async () => {
    mockedView.mockResolvedValueOnce(view());
    renderWithClient(<OwnerPriority />);

    const computed = await screen.findByText(/^Computed\b/);
    expect(computed.querySelector("time")).toHaveAttribute(
      "datetime",
      "2026-10-13T12:00:00Z",
    );
    expect(screen.getByText(/14 users’ lists contribute/)).toBeInTheDocument();
  });

  it("explains an empty order rather than showing an empty table", async () => {
    mockedView.mockResolvedValueOnce(
      view({ ranked: [], contributing_rankers: 0 }),
    );
    renderWithClient(<OwnerPriority />);

    expect(
      await screen.findByText(/No item has been ranked yet/),
    ).toBeInTheDocument();
    expect(screen.queryByRole("table", { name: /priority order/i })).toBeNull();
  });
});

describe("uncertaintyBand", () => {
  it("bands the fit's standard error into low, medium and high", () => {
    expect(uncertaintyBand(0)).toBe("low");
    expect(uncertaintyBand(0.24)).toBe("low");
    expect(uncertaintyBand(0.25)).toBe("medium");
    expect(uncertaintyBand(0.59)).toBe("medium");
    expect(uncertaintyBand(0.6)).toBe("high");
    expect(uncertaintyBand(Number.POSITIVE_INFINITY)).toBe("high");
    expect(uncertaintyBand(Number.NaN)).toBe("high");
  });
});

describe("Board-note editor (FR-FBR-58)", () => {
  async function openEditor(code = "FB-AAAAAA") {
    mockedView.mockResolvedValueOnce(view());
    const user = userEvent.setup();
    renderWithClient(<OwnerPriority />);
    await user.click(
      await screen.findByRole("button", {
        name: `Edit the board note for ${code}`,
      }),
    );
    const region = await screen.findByRole("region", {
      name: `Board note for ${code}`,
    });
    return { user, region };
  }

  it("loads the stored note into the editor", async () => {
    mockedGetNote.mockResolvedValueOnce(note("Starting this in November."));
    const { region } = await openEditor();

    expect(mockedGetNote).toHaveBeenCalledWith("FB-AAAAAA");
    expect(
      await within(region).findByDisplayValue("Starting this in November."),
    ).toBeInTheDocument();
  });

  it("sets a note through the client and shows what the server stored", async () => {
    mockedGetNote.mockResolvedValueOnce(note(null));
    mockedPutNote.mockResolvedValueOnce(note("Shipping next week."));
    const { user, region } = await openEditor();

    expect(await within(region).findByText("No board note yet.")).toBeInTheDocument();
    const box = within(region).getByRole("textbox", { name: "Board note" });
    const save = within(region).getByRole("button", { name: "Save note" });
    // An empty or whitespace note is not a note: the server refuses it (1..=4000).
    expect(save).toBeDisabled();
    await user.type(box, "  Shipping next week.  ");
    await user.click(save);

    expect(mockedPutNote).toHaveBeenCalledWith(
      "FB-AAAAAA",
      "Shipping next week.",
    );
    await waitFor(() =>
      expect(within(region).getByText(/^Last updated /)).toBeInTheDocument(),
    );
    expect(box).toHaveValue("Shipping next week.");
  });

  it("edits an existing note", async () => {
    mockedGetNote.mockResolvedValueOnce(note("Old text"));
    mockedPutNote.mockResolvedValueOnce(note("New text"));
    const { user, region } = await openEditor();

    const box = await within(region).findByDisplayValue("Old text");
    await user.clear(box);
    await user.type(box, "New text");
    await user.click(within(region).getByRole("button", { name: "Save note" }));
    expect(mockedPutNote).toHaveBeenCalledWith("FB-AAAAAA", "New text");
    await waitFor(() => expect(box).toHaveValue("New text"));
  });

  it("clears a note through the client", async () => {
    mockedGetNote.mockResolvedValueOnce(note("Starting this in November."));
    mockedDeleteNote.mockResolvedValueOnce(note(null));
    const { user, region } = await openEditor();

    await within(region).findByDisplayValue("Starting this in November.");
    await user.click(within(region).getByRole("button", { name: "Clear note" }));

    expect(mockedDeleteNote).toHaveBeenCalledWith("FB-AAAAAA");
    expect(await within(region).findByText("No board note yet.")).toBeInTheDocument();
    expect(within(region).getByRole("textbox", { name: "Board note" })).toHaveValue("");
    expect(within(region).queryByRole("button", { name: "Clear note" })).toBeNull();
  });

  it("says so when the item is not on the board (404)", async () => {
    mockedGetNote.mockRejectedValueOnce(
      Object.assign(new Error("Not Found"), {
        isAxiosError: true,
        response: { status: 404 },
      }),
    );
    const { region } = await openEditor();

    expect(
      await within(region).findByText(/not on the board/),
    ).toBeInTheDocument();
    expect(within(region).queryByRole("textbox")).toBeNull();
  });
});
