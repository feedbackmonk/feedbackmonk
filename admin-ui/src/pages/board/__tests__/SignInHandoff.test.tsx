import { describe, expect, it, vi, beforeEach } from "vitest";
import axios from "axios";
import { screen, waitFor } from "@testing-library/react";
import { SignInHandoff, readFragmentToken } from "../SignInHandoff";
import { renderWithClient } from "../../../test/testUtils";

vi.mock("../../../shared/ApiClient", () => ({
  exchangeBoardSession: vi.fn(),
}));

import { exchangeBoardSession } from "../../../shared/ApiClient";
const mockedExchange = vi.mocked(exchangeBoardSession);

const PROJECT_ID = "00000000-0000-0000-0000-000000000abc";
const SIGN_IN = `/public/projects/${PROJECT_ID}/board/sign-in`;
const BOARD = `/public/projects/${PROJECT_ID}/board`;

function renderAt(path: string) {
  return renderWithClient(<SignInHandoff projectId={PROJECT_ID} />, {
    withRouter: true,
    initialPath: path,
  });
}

describe("readFragmentToken", () => {
  it("reads the token from the fragment only", () => {
    expect(readFragmentToken("#token=abc.def.ghi")).toBe("abc.def.ghi");
    expect(readFragmentToken("token=x")).toBe("x");
    expect(readFragmentToken("")).toBeNull();
    expect(readFragmentToken("#token=")).toBeNull();
    expect(readFragmentToken("#other=1")).toBeNull();
  });
});

describe("SignInHandoff (FR-FBR-44)", () => {
  beforeEach(() => {
    mockedExchange.mockReset();
  });

  it("posts the fragment token once, clears the fragment, and goes on to the board", async () => {
    mockedExchange.mockResolvedValueOnce({
      signed_in: true,
      expires_at: "2026-10-13T20:00:00Z",
    });
    renderAt(`${SIGN_IN}#token=header.payload.sig`);

    await waitFor(() => {
      expect(window.location.pathname).toBe(BOARD);
    });
    expect(mockedExchange).toHaveBeenCalledTimes(1);
    expect(mockedExchange).toHaveBeenCalledWith(PROJECT_ID, "header.payload.sig");
    expect(window.location.hash).toBe("");
    expect(window.location.href).not.toContain("header.payload.sig");
  });

  it("clears the fragment even when the exchange fails, and says so", async () => {
    const err = new axios.AxiosError("unauthorized");
    // @ts-expect-error — minimal AxiosResponse stub.
    err.response = { status: 401, data: { error: "InvalidToken" } };
    mockedExchange.mockRejectedValueOnce(err);
    renderAt(`${SIGN_IN}#token=expired.token.sig`);

    expect(
      await screen.findByText(/this sign-in link didn’t work/i),
    ).toBeInTheDocument();
    expect(window.location.hash).toBe("");
    expect(window.location.pathname).toBe(SIGN_IN);
    expect(
      screen.getByRole("link", { name: "Continue to the board without signing in" }),
    ).toHaveAttribute("href", BOARD);
  });

  it("never calls the exchange without a fragment token", async () => {
    renderAt(`${SIGN_IN}?token=from-the-query`);

    expect(
      await screen.findByText(/this sign-in link is incomplete/i),
    ).toBeInTheDocument();
    expect(mockedExchange).not.toHaveBeenCalled();
  });
});
