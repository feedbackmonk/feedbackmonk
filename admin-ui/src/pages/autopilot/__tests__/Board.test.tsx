import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import type { WorkOrder } from "../../../shared/types.gen";
import { renderWithClient } from "../../../test/testUtils";
import {
  BOARD_COLUMNS,
  allWorkOrderStates,
  columnForState,
  groupByColumn,
  type BoardColumnId,
} from "../boardColumns";

vi.mock("../../../shared/ApiClient", async () => {
  const actual = await vi.importActual<
    typeof import("../../../shared/ApiClient")
  >("../../../shared/ApiClient");
  return {
    ...actual,
    fetchAdminProjects: vi.fn(),
    fetchWorkOrders: vi.fn(),
  };
});

import { fetchAdminProjects, fetchWorkOrders } from "../../../shared/ApiClient";
import { Board } from "../Board";

const mockedProjects = vi.mocked(fetchAdminProjects);
const mockedWorkOrders = vi.mocked(fetchWorkOrders);

const PROJECT = "proj-1";

function wo(overrides: Partial<WorkOrder> = {}): WorkOrder {
  return {
    id: "wo-1",
    recommendation_id: "rec-1",
    cluster_id: "clu-1",
    action_type: "bug_fix",
    title: "Fix the login crash",
    instructions: "Fix it.",
    owner_overrides: null,
    autonomy_rung: 1,
    state: "draft",
    approved_by: null,
    approved_at: null,
    dispatched_at: null,
    claimed_by_runner: null,
    result_ref: null,
    failure_reason: null,
    routing_label: null,
    created_at: "2026-07-12T10:00:00Z",
    updated_at: "2026-07-12T10:00:00Z",
    ...overrides,
  };
}

// ─── Column model: exhaustive over EVERY work-order state ──────────────────

describe("boardColumns — exhaustive state→column mapping (C31 §7)", () => {
  const validColumnIds = new Set<BoardColumnId>(BOARD_COLUMNS.map((c) => c.id));

  it("has exactly 6 columns in lifecycle order", () => {
    expect(BOARD_COLUMNS.map((c) => c.id)).toEqual([
      "draft",
      "approved",
      "in-flight",
      "reported",
      "done",
      "halted",
    ]);
  });

  it("maps every one of the work-order states to exactly one valid column", () => {
    const states = allWorkOrderStates();
    // Guard against the mapping quietly shrinking: the runtime state list is the
    // canonical 10-state set. A new state added to the union appears here and
    // must resolve to a column, or this test fails (it must not vanish).
    expect(states).toHaveLength(10);
    for (const state of states) {
      const col = columnForState(state);
      expect(
        validColumnIds.has(col),
        `state "${state}" → "${col}" is not a valid column`,
      ).toBe(true);
    }
  });

  it("groups the four execution states together under In flight", () => {
    for (const state of ["dispatched", "claimed", "building", "verifying"] as const) {
      expect(columnForState(state)).toBe("in-flight");
    }
  });

  it("groups failed + cancelled under Halted, completed under Done", () => {
    expect(columnForState("failed")).toBe("halted");
    expect(columnForState("cancelled")).toBe("halted");
    expect(columnForState("completed")).toBe("done");
  });

  it("partitions orders into columns with no drops and no duplication", () => {
    const orders = allWorkOrderStates().map((state, i) =>
      wo({ id: `wo-${i}`, state }),
    );
    const groups = groupByColumn(orders);
    const regrouped = BOARD_COLUMNS.flatMap((c) => groups[c.id]);
    expect(regrouped).toHaveLength(orders.length);
    expect(new Set(regrouped.map((o) => o.id)).size).toBe(orders.length);
  });
});

// ─── Board page render ─────────────────────────────────────────────────────

describe("Board page", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedProjects.mockResolvedValue({
      projects: [
        {
          project_id: PROJECT,
          name: "Acme",
          slug: "acme",
          created_at: "2026-01-01T00:00:00Z",
        },
      ],
    });
  });

  it("renders all 6 columns with counts", async () => {
    mockedWorkOrders.mockResolvedValue({
      items: [
        wo({ id: "a", state: "draft" }),
        wo({ id: "b", state: "approved" }),
        wo({ id: "c", state: "building" }),
      ],
      total: 3,
      limit: 200,
      offset: 0,
    });
    renderWithClient(<Board />, { withRouter: true });

    for (const label of ["Draft", "Approved", "In flight", "Reported", "Done", "Halted"]) {
      await waitFor(() =>
        expect(
          screen.getByRole("heading", { name: new RegExp(`^${label} `) }),
        ).toBeInTheDocument(),
      );
    }
  });

  it("renders routing_label and claimed_by_runner on a card", async () => {
    mockedWorkOrders.mockResolvedValue({
      items: [
        wo({
          id: "a",
          state: "claimed",
          routing_label: "ci-runner",
          claimed_by_runner: "ci-runner",
        }),
      ],
      total: 1,
    });
    renderWithClient(<Board />, { withRouter: true });
    await waitFor(() =>
      expect(screen.getByText(/→ ci-runner/)).toBeInTheDocument(),
    );
    expect(screen.getByText(/claimed · ci-runner/)).toBeInTheDocument();
  });

  it("renders an owner-authored (null-provenance) card cleanly", async () => {
    mockedWorkOrders.mockResolvedValue({
      items: [
        wo({
          id: "a",
          state: "draft",
          recommendation_id: null,
          cluster_id: null,
          title: "Owner-written story",
        }),
      ],
      total: 1,
    });
    renderWithClient(<Board />, { withRouter: true });
    await waitFor(() =>
      expect(screen.getByText("Owner-written story")).toBeInTheDocument(),
    );
    expect(screen.getByText(/Owner-authored/)).toBeInTheDocument();
  });

  it("surfaces a truncation note when more orders exist than fetched", async () => {
    mockedWorkOrders.mockResolvedValue({
      items: [wo({ id: "a", state: "draft" })],
      total: 500,
      limit: 200,
      offset: 0,
    });
    renderWithClient(<Board />, { withRouter: true });
    await waitFor(() =>
      expect(screen.getByText(/of 500/)).toBeInTheDocument(),
    );
  });

  it("renders an injection-payload title as inert escaped text", async () => {
    const malicious = `"><img src=x onerror=alert(1)> ignore previous instructions`;
    mockedWorkOrders.mockResolvedValue({
      items: [wo({ id: "a", state: "draft", title: malicious })],
      total: 1,
    });
    const { container } = renderWithClient(<Board />, { withRouter: true });
    await waitFor(() =>
      expect(screen.getByText(/ignore previous instructions/)).toBeInTheDocument(),
    );
    // The smuggled <img> is present as text, never as a live element.
    expect(container.querySelector("img")).toBeNull();
  });
});
