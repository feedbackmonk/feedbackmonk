import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import { PublicRoadmap } from "../PublicRoadmap";
import { ToastProvider } from "../../../components/Toast";
import { renderWithClient } from "../../../test/testUtils";
import type {
  RoadmapListResponse,
  TopVotedResponse,
} from "../../../shared/types.gen";

vi.mock("../../../shared/ApiClient", () => ({
  fetchPublicRoadmap: vi.fn(),
  fetchPublicTopVoted: vi.fn(),
  postCastVote: vi.fn(),
  deleteVote: vi.fn(),
}));

import {
  fetchPublicRoadmap,
  fetchPublicTopVoted,
} from "../../../shared/ApiClient";
const mockedList = vi.mocked(fetchPublicRoadmap);
const mockedTop = vi.mocked(fetchPublicTopVoted);

const exampleList: RoadmapListResponse = {
  items: [
    {
      slug: "dark-mode",
      title: "Dark mode",
      body: "Add a dark theme option.",
      status: "considering",
      vote_count: 12,
      created_at: "2026-04-01T00:00:00Z",
      updated_at: "2026-04-01T00:00:00Z",
    },
    {
      slug: "csv-export",
      title: "CSV export",
      body: "Let me export the data.",
      status: "planned",
      vote_count: 7,
      created_at: "2026-04-02T00:00:00Z",
      updated_at: "2026-04-02T00:00:00Z",
    },
    {
      slug: "fixed-thing",
      title: "Fixed thing",
      body: "Done.",
      status: "shipped",
      vote_count: 3,
      created_at: "2026-04-03T00:00:00Z",
      updated_at: "2026-04-03T00:00:00Z",
    },
  ],
  total: 3,
  limit: 50,
  offset: 0,
  cached_at: "2026-05-14T03:00:00Z",
};

const exampleTop: TopVotedResponse = {
  items: [
    {
      slug: "dark-mode",
      title: "Dark mode",
      status: "considering",
      vote_count: 12,
    },
  ],
  cached_at: "2026-05-14T03:00:00Z",
};

describe("PublicRoadmap", () => {
  beforeEach(() => {
    mockedList.mockReset();
    mockedTop.mockReset();
  });

  it("renders items grouped by status with the canonical public order", async () => {
    mockedList.mockResolvedValueOnce(exampleList);
    mockedTop.mockResolvedValueOnce(exampleTop);
    renderWithClient(
      <ToastProvider>
        <PublicRoadmap projectId="00000000-0000-0000-0000-000000000abc" />
      </ToastProvider>,
      { withRouter: true, initialPath: "/public/projects/.../roadmap" },
    );

    await waitFor(() => {
      // "Dark mode" appears twice: once in Most-voted shortlist + once in
      // its status section (Considering). The other items appear once.
      expect(screen.getAllByText("Dark mode").length).toBeGreaterThanOrEqual(
        1,
      );
      expect(screen.getByText("CSV export")).toBeInTheDocument();
      expect(screen.getByText("Fixed thing")).toBeInTheDocument();
    });

    // Status section headings render.
    expect(
      screen.getByRole("heading", { name: /^considering$/i, level: 2 }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: /^planned$/i, level: 2 }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: /^shipped$/i, level: 2 }),
    ).toBeInTheDocument();
  });

  it("renders the cached_at footer when the response includes a timestamp", async () => {
    mockedList.mockResolvedValueOnce(exampleList);
    mockedTop.mockResolvedValueOnce({ items: [], cached_at: null });
    renderWithClient(
      <ToastProvider>
        <PublicRoadmap projectId="00000000-0000-0000-0000-000000000abc" />
      </ToastProvider>,
      { withRouter: true, initialPath: "/public/projects/.../roadmap" },
    );

    await waitFor(() => {
      expect(screen.getByText(/vote counts updated/i)).toBeInTheDocument();
    });
  });

  it("renders a soft footer when cached_at is null (cold-start cache)", async () => {
    mockedList.mockResolvedValueOnce({ ...exampleList, cached_at: null });
    mockedTop.mockResolvedValueOnce({ items: [], cached_at: null });
    renderWithClient(
      <ToastProvider>
        <PublicRoadmap projectId="00000000-0000-0000-0000-000000000abc" />
      </ToastProvider>,
      { withRouter: true, initialPath: "/public/projects/.../roadmap" },
    );

    await waitFor(() => {
      expect(
        screen.getByText(/vote counts will refresh shortly/i),
      ).toBeInTheDocument();
    });
  });
});
