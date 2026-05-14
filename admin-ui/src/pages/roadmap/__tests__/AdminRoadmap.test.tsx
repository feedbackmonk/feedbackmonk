import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AdminRoadmap } from "../AdminRoadmap";
import { ToastProvider } from "../../../components/Toast";
import { renderWithClient } from "../../../test/testUtils";
import type {
  AdminProjectListResponse,
} from "../../../shared/ApiClient";
import type {
  RoadmapItem,
  RoadmapListResponse,
} from "../../../shared/types.gen";

vi.mock("../../../shared/ApiClient", () => ({
  fetchAdminProjects: vi.fn(),
  fetchAdminRoadmap: vi.fn(),
  patchRoadmapItem: vi.fn(),
  postCreateRoadmapItem: vi.fn(),
}));

import {
  fetchAdminProjects,
  fetchAdminRoadmap,
  patchRoadmapItem,
} from "../../../shared/ApiClient";
const mockedProjects = vi.mocked(fetchAdminProjects);
const mockedList = vi.mocked(fetchAdminRoadmap);
const mockedPatch = vi.mocked(patchRoadmapItem);

const exampleProjects: AdminProjectListResponse = {
  projects: [
    {
      project_id: "00000000-0000-0000-0000-000000000abc",
      name: "Test Project",
      slug: "test-project",
      created_at: "2026-04-01T00:00:00Z",
    },
  ],
};

const item: RoadmapItem = {
  slug: "dark-mode",
  title: "Dark mode",
  body: "Add a dark theme option.",
  status: "considering",
  vote_count: 5,
  created_at: "2026-04-01T00:00:00Z",
  updated_at: "2026-04-01T00:00:00Z",
};

const exampleList: RoadmapListResponse = {
  items: [item],
  total: 1,
  limit: 100,
  offset: 0,
  cached_at: null,
};

describe("AdminRoadmap", () => {
  beforeEach(() => {
    mockedProjects.mockReset();
    mockedList.mockReset();
    mockedPatch.mockReset();
  });

  it("resolves sole-project then renders the list grouped by status", async () => {
    mockedProjects.mockResolvedValueOnce(exampleProjects);
    mockedList.mockResolvedValueOnce(exampleList);
    renderWithClient(
      <ToastProvider>
        <AdminRoadmap />
      </ToastProvider>,
      { withRouter: true, initialPath: "/admin/roadmap" },
    );

    await waitFor(() => {
      expect(screen.getByText("Dark mode")).toBeInTheDocument();
    });
    expect(mockedList).toHaveBeenCalledWith(
      "00000000-0000-0000-0000-000000000abc",
    );
  });

  it("clicking Edit opens dialog; status change fires PATCH against the resolved project + slug", async () => {
    mockedProjects.mockResolvedValueOnce(exampleProjects);
    mockedList.mockResolvedValue(exampleList);
    mockedPatch.mockResolvedValueOnce({ ...item, status: "planned" });
    const user = userEvent.setup();
    renderWithClient(
      <ToastProvider>
        <AdminRoadmap />
      </ToastProvider>,
      { withRouter: true, initialPath: "/admin/roadmap" },
    );

    await waitFor(() => expect(screen.getByText("Dark mode")).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: /edit/i }));

    // Status select inside the dialog.
    const statusSelect = await screen.findByLabelText(/^status$/i);
    await user.selectOptions(statusSelect, "planned");
    await user.click(screen.getByRole("button", { name: /^save$/i }));

    await waitFor(() => {
      expect(mockedPatch).toHaveBeenCalledWith(
        "00000000-0000-0000-0000-000000000abc",
        "dark-mode",
        expect.objectContaining({ status: "planned" }),
      );
    });
  });
});
