import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen, within, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RunnerTokens } from "../RunnerTokens";
import { tokenLifecycle } from "../RunnerTokenCard";
import type {
  RunnerTokenListResponse,
  RunnerTokenView,
} from "../../../shared/types.gen";
import { renderWithClient } from "../../../test/testUtils";

vi.mock("../../../shared/ApiClient", async () => {
  const actual = await vi.importActual<
    typeof import("../../../shared/ApiClient")
  >("../../../shared/ApiClient");
  return {
    ...actual,
    fetchAdminProjects: vi.fn(),
    listRunnerTokens: vi.fn(),
    revokeRunnerToken: vi.fn(),
    registerRunnerToken: vi.fn(),
    registerSigningKey: vi.fn(),
  };
});

import {
  fetchAdminProjects,
  listRunnerTokens,
  revokeRunnerToken,
} from "../../../shared/ApiClient";

const mockedProjects = vi.mocked(fetchAdminProjects);
const mockedList = vi.mocked(listRunnerTokens);
const mockedRevoke = vi.mocked(revokeRunnerToken);

const PROJECT_ID = "11111111-1111-1111-1111-111111111111";

function token(overrides: Partial<RunnerTokenView> = {}): RunnerTokenView {
  return {
    jti: "jti-active",
    label: "ci-runner",
    expires_at: "2099-01-01T00:00:00Z",
    created_at: "2026-06-18T00:00:00Z",
    revoked_at: null,
    ...overrides,
  };
}

function listResponse(items: RunnerTokenView[]): RunnerTokenListResponse {
  return { items };
}

beforeEach(() => {
  mockedProjects.mockReset();
  mockedList.mockReset();
  mockedRevoke.mockReset();
  mockedProjects.mockResolvedValue({
    projects: [
      {
        project_id: PROJECT_ID,
        name: "Demo",
        slug: "demo",
        created_at: "2026-01-01T00:00:00Z",
      },
    ],
  });
});

describe("tokenLifecycle — derives status from server fields", () => {
  const now = new Date("2030-01-01T00:00:00Z");

  it("revoked wins over everything", () => {
    expect(
      tokenLifecycle(
        token({ revoked_at: "2026-06-19T00:00:00Z", expires_at: null }),
        now,
      ),
    ).toBe("revoked");
  });

  it("expired when exp is in the past and not revoked", () => {
    expect(
      tokenLifecycle(token({ expires_at: "2026-06-18T00:00:00Z" }), now),
    ).toBe("expired");
  });

  it("active when not revoked and exp is in the future (or absent)", () => {
    expect(tokenLifecycle(token({ expires_at: null }), now)).toBe("active");
    expect(
      tokenLifecycle(token({ expires_at: "2099-01-01T00:00:00Z" }), now),
    ).toBe("active");
  });
});

describe("RunnerTokens — page render", () => {
  it("surfaces the can't-author-approved security property", async () => {
    mockedList.mockResolvedValue(listResponse([]));
    renderWithClient(<RunnerTokens />, {
      initialPath: "/admin/settings/runner-tokens",
    });

    // The page heading renders in the loading state too, so await the
    // security copy itself (only present once the project resolves).
    expect(
      await screen.findByText(/never approve one/i),
    ).toBeInTheDocument();
  });

  it("empty token list shows the 'optional' empty state", async () => {
    mockedList.mockResolvedValue(listResponse([]));
    renderWithClient(<RunnerTokens />, {
      initialPath: "/admin/settings/runner-tokens",
    });

    expect(
      await screen.findByText(/No tokens registered yet/i),
    ).toBeInTheDocument();
  });

  it("renders a registered token with its label, jti, and an active status", async () => {
    mockedList.mockResolvedValue(listResponse([token()]));
    renderWithClient(<RunnerTokens />, {
      initialPath: "/admin/settings/runner-tokens",
    });

    const card = (await screen.findByText("jti-active")).closest(
      ".runner-token-card",
    ) as HTMLElement;
    expect(within(card).getByText("ci-runner")).toBeInTheDocument();
    expect(within(card).getByText("Active")).toBeInTheDocument();
  });

  it("a revoked token shows Revoked status and a disabled revoke button", async () => {
    mockedList.mockResolvedValue(
      listResponse([
        token({
          jti: "jti-dead",
          label: "old-runner",
          revoked_at: "2026-06-19T00:00:00Z",
        }),
      ]),
    );
    renderWithClient(<RunnerTokens />, {
      initialPath: "/admin/settings/runner-tokens",
    });

    const card = (await screen.findByText("jti-dead")).closest(
      ".runner-token-card",
    ) as HTMLElement;
    // "Revoked" appears in the status badge, the metadata label, and the
    // button — scope the status assertion to the badge.
    expect(
      within(card).getByText("Revoked", { selector: ".runner-token-status" }),
    ).toBeInTheDocument();
    const btn = within(card).getByRole("button", { name: /Revoked/i });
    expect(btn).toBeDisabled();
  });

  it("revoke action calls the API with the token's jti (confirm accepted)", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
    mockedList.mockResolvedValue(listResponse([token({ jti: "jti-x" })]));
    mockedRevoke.mockResolvedValue(undefined);
    renderWithClient(<RunnerTokens />, {
      initialPath: "/admin/settings/runner-tokens",
    });

    const card = (await screen.findByText("jti-x")).closest(
      ".runner-token-card",
    ) as HTMLElement;
    await userEvent.click(within(card).getByRole("button", { name: /^Revoke$/ }));

    await waitFor(() =>
      expect(mockedRevoke).toHaveBeenCalledWith(PROJECT_ID, "jti-x"),
    );
  });
});
