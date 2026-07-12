import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { WorkOrderDetail, WorkOrderState } from "../../../shared/types.gen";
import { renderWithClient } from "../../../test/testUtils";

vi.mock("../../../shared/ApiClient", async () => {
  const actual = await vi.importActual<
    typeof import("../../../shared/ApiClient")
  >("../../../shared/ApiClient");
  return {
    ...actual,
    fetchAdminProjects: vi.fn(),
    fetchWorkOrderDetail: vi.fn(),
    transitionWorkOrder: vi.fn(),
  };
});

import {
  fetchAdminProjects,
  fetchWorkOrderDetail,
  transitionWorkOrder,
} from "../../../shared/ApiClient";
import { WorkOrderDetail as WorkOrderDetailPage } from "../WorkOrderDetail";

const mockedProjects = vi.mocked(fetchAdminProjects);
const mockedDetail = vi.mocked(fetchWorkOrderDetail);
const mockedTransition = vi.mocked(transitionWorkOrder);

function wo(state: WorkOrderState): WorkOrderDetail {
  return {
    id: "wo-1",
    recommendation_id: "rec-1",
    cluster_id: "clu-1",
    action_type: "bug_fix",
    title: "Fix the login crash",
    state,
    autonomy_rung: 1,
    approved_at: state === "draft" ? null : "2026-06-18T11:00:00Z",
    created_at: "2026-06-18T10:00:00Z",
    updated_at: "2026-06-18T11:00:00Z",
    instructions: "Fix the Safari login button.",
    owner_overrides: null,
    approved_by: "admin@example.com",
    dispatched_at: null,
    claimed_by_runner: null,
    result_ref: null,
    failure_reason: null,
    routing_label: null,
    events: [
      {
        id: "ev-1",
        from_state: null,
        to_state: "draft",
        event_type: "create",
        actor: "admin",
        actor_id: "admin@example.com",
        detail: null,
        at: "2026-06-18T10:00:00Z",
      },
    ],
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mockedProjects.mockResolvedValue({
    projects: [
      { project_id: "proj-1", name: "P", slug: "p", created_at: "2026-01-01" },
    ],
  });
  mockedTransition.mockResolvedValue({
    work_order_id: "wo-1",
    from_state: "reported",
    to_state: "building",
    event_type: "request-changes",
    audit_id: "audit-1",
  });
});

describe("WorkOrderDetail — owner actions mirror the C22 authz table", () => {
  it("offers accept / request-changes / reject on a reported order", async () => {
    mockedDetail.mockResolvedValue(wo("reported"));
    renderWithClient(<WorkOrderDetailPage workOrderId="wo-1" />, { withRouter: true });

    await waitFor(() =>
      expect(screen.getByRole("heading", { name: /Fix the login crash/ })).toBeInTheDocument(),
    );
    expect(screen.getByRole("button", { name: /Accept result/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Request changes/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Reject$/ })).toBeInTheDocument();
  });

  it("never offers Approve here — approval is the rec-card /approve gate, not a generic transition", async () => {
    mockedDetail.mockResolvedValue(wo("draft"));
    renderWithClient(<WorkOrderDetailPage workOrderId="wo-1" />, { withRouter: true });

    await waitFor(() =>
      expect(screen.getByRole("button", { name: /Cancel order/ })).toBeInTheDocument(),
    );
    // A draft work order's only owner action is cancel — `approve` is NOT a
    // transition button (it routes through the dedicated /approve security gate).
    expect(screen.queryByRole("button", { name: /^Approve/ })).toBeNull();
  });

  it("shows no actions on a terminal order", async () => {
    mockedDetail.mockResolvedValue(wo("completed"));
    renderWithClient(<WorkOrderDetailPage workOrderId="wo-1" />, { withRouter: true });

    await waitFor(() =>
      expect(screen.getByText(/terminal, no further actions/i)).toBeInTheDocument(),
    );
  });

  it("request-changes carries an authoritative overrides delta", async () => {
    mockedDetail.mockResolvedValue(wo("reported"));
    const user = userEvent.setup();
    renderWithClient(<WorkOrderDetailPage workOrderId="wo-1" />, { withRouter: true });

    await waitFor(() =>
      expect(screen.getByRole("button", { name: /Request changes/ })).toBeInTheDocument(),
    );
    await user.click(screen.getByRole("button", { name: /Request changes/ }));

    const dialog = await screen.findByRole("dialog", { name: /Request changes/ });
    const titleInput = within(dialog).getByLabelText(/^Title$/);
    await user.clear(titleInput);
    await user.type(titleInput, "Narrow to Safari 17 only");
    await user.click(
      within(dialog).getByRole("button", { name: /^Request changes$/ }),
    );

    await waitFor(() => expect(mockedTransition).toHaveBeenCalledTimes(1));
    const body = mockedTransition.mock.calls[0][2];
    expect(body.event_type).toBe("request-changes");
    expect(
      (body.detail?.owner_overrides as { title?: string } | undefined)?.title,
    ).toBe("Narrow to Safari 17 only");
  });

  it("renders the append-only event ledger", async () => {
    mockedDetail.mockResolvedValue(wo("approved"));
    renderWithClient(<WorkOrderDetailPage workOrderId="wo-1" />, { withRouter: true });
    const ledger = await screen.findByRole("heading", { name: /Event ledger/ });
    expect(ledger).toBeInTheDocument();
    // The creating event renders its raw event_type ("create" has no display
    // label) — a unique marker that the append-only ledger rendered.
    expect(screen.getByText(/create/)).toBeInTheDocument();
  });
});
