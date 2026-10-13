import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { BoardSettings } from "../BoardSettings";
import type { BoardSettings as BoardSettingsShape } from "../../../shared/boardModerationApi";
import { renderWithClient } from "../../../test/testUtils";

// The board-access setting (FR-FBR-43, Contract C44): the owner chooses who may
// see an enabled board, and the choice round-trips through the PATCH leg of
// `/admin/projects/{id}/board-settings`.

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
    fetchBoardSettings: vi.fn(),
    patchBoardSettings: vi.fn(),
  };
});

import { fetchAdminProjects } from "../../../shared/ApiClient";
import {
  fetchBoardSettings,
  patchBoardSettings,
} from "../../../shared/boardModerationApi";

const mockedProjects = vi.mocked(fetchAdminProjects);
const mockedFetch = vi.mocked(fetchBoardSettings);
const mockedPatch = vi.mocked(patchBoardSettings);

const PROJECT_ID = "11111111-1111-1111-1111-111111111111";

function settings(
  overrides: Partial<BoardSettingsShape> = {},
): BoardSettingsShape {
  return {
    public_board_enabled: true,
    board_requires_moderation: true,
    board_access: "public",
    ...overrides,
  };
}

beforeEach(() => {
  mockedProjects.mockReset();
  mockedFetch.mockReset();
  mockedPatch.mockReset();
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

describe("BoardSettings — board access (FR-FBR-43)", () => {
  it("shows the stored access as the checked choice", async () => {
    mockedFetch.mockResolvedValueOnce(settings({ board_access: "signed_in" }));
    renderWithClient(<BoardSettings />);

    const signedIn = await screen.findByRole("radio", {
      name: "Signed-in users only",
    });
    expect(signedIn).toBeChecked();
    expect(
      screen.getByRole("radio", { name: "Anyone with the link" }),
    ).not.toBeChecked();
    expect(
      screen.getByRole("group", { name: "Who can see the board" }),
    ).toBeInTheDocument();
  });

  it("round-trips a change of access through the PATCH and shows the server's answer", async () => {
    mockedFetch.mockResolvedValueOnce(settings({ board_access: "public" }));
    mockedPatch.mockResolvedValueOnce(settings({ board_access: "signed_in" }));
    const user = userEvent.setup();
    renderWithClient(<BoardSettings />);

    const signedIn = await screen.findByRole("radio", {
      name: "Signed-in users only",
    });
    expect(signedIn).not.toBeChecked();
    await user.click(signedIn);

    // Only the access leg is sent: the other toggles keep their own PATCH.
    expect(mockedPatch).toHaveBeenCalledWith(PROJECT_ID, {
      board_access: "signed_in",
    });
    await waitFor(() =>
      expect(
        screen.getByRole("radio", { name: "Signed-in users only" }),
      ).toBeChecked(),
    );
  });

  it("keeps the stored choice when the server refuses the change", async () => {
    mockedFetch.mockResolvedValueOnce(settings({ board_access: "public" }));
    mockedPatch.mockRejectedValueOnce(new Error("400"));
    const user = userEvent.setup();
    renderWithClient(<BoardSettings />);

    await user.click(
      await screen.findByRole("radio", { name: "Signed-in users only" }),
    );
    await waitFor(() => expect(mockedPatch).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(
        screen.getByRole("radio", { name: "Anyone with the link" }),
      ).toBeChecked(),
    );
  });

  it("treats a response without board_access as public, the default", async () => {
    const legacy = settings();
    delete (legacy as Partial<BoardSettingsShape>).board_access;
    mockedFetch.mockResolvedValueOnce(legacy);
    renderWithClient(<BoardSettings />);

    expect(
      await screen.findByRole("radio", { name: "Anyone with the link" }),
    ).toBeChecked();
  });

  it("still saves the enable toggle on its own leg", async () => {
    mockedFetch.mockResolvedValueOnce(settings({ public_board_enabled: false }));
    mockedPatch.mockResolvedValueOnce(settings({ public_board_enabled: true }));
    const user = userEvent.setup();
    renderWithClient(<BoardSettings />);

    await user.click(
      await screen.findByRole("checkbox", { name: "Enable public board" }),
    );
    expect(mockedPatch).toHaveBeenCalledWith(PROJECT_ID, {
      public_board_enabled: true,
    });
  });
});
