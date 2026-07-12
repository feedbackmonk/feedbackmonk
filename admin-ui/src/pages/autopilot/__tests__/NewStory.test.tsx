import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type {
  CreateOwnerWorkOrderRequest,
  WorkOrderDetail,
} from "../../../shared/types.gen";
import { renderWithClient } from "../../../test/testUtils";

vi.mock("../../../shared/ApiClient", async () => {
  const actual = await vi.importActual<
    typeof import("../../../shared/ApiClient")
  >("../../../shared/ApiClient");
  return {
    ...actual,
    fetchAdminProjects: vi.fn(),
    createWorkOrder: vi.fn(),
    approveWorkOrder: vi.fn(),
  };
});

import {
  fetchAdminProjects,
  createWorkOrder,
  approveWorkOrder,
} from "../../../shared/ApiClient";
import { NewStory } from "../NewStory";

const mockedProjects = vi.mocked(fetchAdminProjects);
const mockedCreate = vi.mocked(createWorkOrder);
const mockedApprove = vi.mocked(approveWorkOrder);

const PROJECT = "proj-1";

// createWorkOrder returns A's WorkOrderView (the created draft). Owner-authored
// ⇒ recommendation_id is null; we only use `id` to navigate to detail.
function draft(): WorkOrderDetail {
  return {
    id: "wo-new",
    recommendation_id: null,
    cluster_id: null,
    action_type: "feature_implementation",
    title: "Add dark mode",
    state: "draft",
    autonomy_rung: 1,
    approved_at: null,
    created_at: "2026-07-12T21:00:00Z",
    updated_at: "2026-07-12T21:00:00Z",
    instructions: "Implement a dark theme toggle.",
    owner_overrides: null,
    approved_by: null,
    dispatched_at: null,
    claimed_by_runner: null,
    result_ref: null,
    failure_reason: null,
    routing_label: null,
    events: [],
  };
}

function render() {
  return renderWithClient(<NewStory />, {
    withRouter: true,
    initialPath: "/admin/autopilot/work-orders/new",
  });
}

async function waitForForm() {
  await waitFor(() =>
    expect(screen.getByLabelText("Title")).toBeInTheDocument(),
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mockedProjects.mockResolvedValue({
    projects: [
      { project_id: PROJECT, name: "P", slug: "p", created_at: "2026-01-01" },
    ],
  });
  mockedCreate.mockResolvedValue(draft());
  window.history.replaceState(null, "", "/admin/autopilot/work-orders/new");
});

describe("NewStory — owner-authored work-order creation (C31 §3)", () => {
  it("composes the owner-authored union variant: no recommendation_id key", async () => {
    const user = userEvent.setup();
    render();
    await waitForForm();

    await user.type(screen.getByLabelText("Title"), "Add dark mode");
    await user.type(
      screen.getByLabelText("Instructions"),
      "Implement a dark theme toggle.",
    );
    await user.click(screen.getByRole("button", { name: /Create draft/i }));

    await waitFor(() => expect(mockedCreate).toHaveBeenCalledTimes(1));
    const [pid, body] = mockedCreate.mock.calls[0] as [
      string,
      CreateOwnerWorkOrderRequest,
    ];
    expect(pid).toBe(PROJECT);
    // The structural discriminant: owner-authored has NO recommendation_id key
    // at all (not `recommendation_id: null`).
    expect(body).not.toHaveProperty("recommendation_id");
    expect(body).not.toHaveProperty("owner_overrides");
    expect(body).toMatchObject({
      title: "Add dark mode",
      instructions: "Implement a dark theme toggle.",
      action_type: "feature_implementation",
      autonomy_rung: 1,
    });
    // Empty routing label is omitted (first-claim-wins), not sent as "".
    expect(body).not.toHaveProperty("routing_label");
  });

  it("NEVER approves — the form only creates a draft", async () => {
    const user = userEvent.setup();
    render();
    await waitForForm();

    await user.type(screen.getByLabelText("Title"), "Add dark mode");
    await user.type(
      screen.getByLabelText("Instructions"),
      "Implement a dark theme toggle.",
    );
    await user.click(screen.getByRole("button", { name: /Create draft/i }));

    await waitFor(() => expect(mockedCreate).toHaveBeenCalledTimes(1));
    // Approval is a separate, deliberate act on the detail page. The create
    // form must never call the approval gate.
    expect(mockedApprove).not.toHaveBeenCalled();
  });

  it("navigates to the created draft's detail page on success", async () => {
    const user = userEvent.setup();
    render();
    await waitForForm();

    await user.type(screen.getByLabelText("Title"), "Add dark mode");
    await user.type(
      screen.getByLabelText("Instructions"),
      "Implement a dark theme toggle.",
    );
    await user.click(screen.getByRole("button", { name: /Create draft/i }));

    await waitFor(() =>
      expect(window.location.pathname).toBe(
        "/admin/autopilot/work-orders/wo-new",
      ),
    );
  });

  it("includes routing_label only when the owner names a runner", async () => {
    const user = userEvent.setup();
    render();
    await waitForForm();

    await user.type(screen.getByLabelText("Title"), "Add dark mode");
    await user.type(
      screen.getByLabelText("Instructions"),
      "Implement a dark theme toggle.",
    );
    await user.type(
      screen.getByLabelText(/Routing label/i),
      "  ci-runner  ",
    );
    await user.click(screen.getByRole("button", { name: /Create draft/i }));

    await waitFor(() => expect(mockedCreate).toHaveBeenCalledTimes(1));
    const body = mockedCreate.mock.calls[0][1] as CreateOwnerWorkOrderRequest;
    // Trimmed and included.
    expect(body.routing_label).toBe("ci-runner");
  });

  it("carries the selected action type", async () => {
    const user = userEvent.setup();
    render();
    await waitForForm();

    await user.type(screen.getByLabelText("Title"), "Fix the crash");
    await user.type(
      screen.getByLabelText("Instructions"),
      "Handle the null pointer on logout.",
    );
    await user.selectOptions(
      screen.getByLabelText("Action type"),
      "bug_fix",
    );
    await user.click(screen.getByRole("button", { name: /Create draft/i }));

    await waitFor(() => expect(mockedCreate).toHaveBeenCalledTimes(1));
    const body = mockedCreate.mock.calls[0][1] as CreateOwnerWorkOrderRequest;
    expect(body.action_type).toBe("bug_fix");
  });

  it("rejects a whitespace-only title without creating anything", async () => {
    const user = userEvent.setup();
    render();
    await waitForForm();

    // Whitespace satisfies the HTML `required` attribute but fails the JS
    // trim() guard — exercising the inline validation path.
    await user.type(screen.getByLabelText("Title"), "   ");
    await user.type(
      screen.getByLabelText("Instructions"),
      "Some real instructions.",
    );
    await user.click(screen.getByRole("button", { name: /Create draft/i }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      /title is required/i,
    );
    expect(mockedCreate).not.toHaveBeenCalled();
  });

  it("rejects whitespace-only instructions without creating anything", async () => {
    const user = userEvent.setup();
    render();
    await waitForForm();

    await user.type(screen.getByLabelText("Title"), "A real title");
    await user.type(screen.getByLabelText("Instructions"), "   ");
    await user.click(screen.getByRole("button", { name: /Create draft/i }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      /instructions are required/i,
    );
    expect(mockedCreate).not.toHaveBeenCalled();
  });
});
