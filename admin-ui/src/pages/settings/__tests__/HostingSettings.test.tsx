import { describe, expect, it, vi, beforeEach } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { HostingSettings } from "../HostingSettings";
import type { HostingSettings as HostingShape } from "../../../shared/hostingApi";
import { renderWithClient } from "../../../test/testUtils";

vi.mock("../../../shared/hostingApi", async () => {
  const actual =
    await vi.importActual<typeof import("../../../shared/hostingApi")>(
      "../../../shared/hostingApi",
    );
  return {
    ...actual,
    fetchHosting: vi.fn(),
    putSubdomain: vi.fn(),
    claimDomain: vi.fn(),
    releaseDomain: vi.fn(),
  };
});

import {
  claimDomain,
  fetchHosting,
  putSubdomain,
} from "../../../shared/hostingApi";

const mockedFetch = vi.mocked(fetchHosting);
const mockedPutSubdomain = vi.mocked(putSubdomain);
const mockedClaim = vi.mocked(claimDomain);

function hosting(overrides: Partial<HostingShape> = {}): HostingShape {
  return {
    tier: "pro",
    subdomain: "acme",
    public_host: "acme.feedbackmonk.com",
    cname_target: "acme.feedbackmonk.com",
    custom_domain_available: true,
    domains: [],
    ...overrides,
  };
}

describe("HostingSettings — tenant subdomain + custom domains (FR-FBR-32/33)", () => {
  beforeEach(() => {
    mockedFetch.mockReset();
    mockedPutSubdomain.mockReset();
    mockedClaim.mockReset();
  });

  it("shows the tenant's public host and the CNAME target to point at it", async () => {
    mockedFetch.mockResolvedValueOnce(hosting());
    renderWithClient(<HostingSettings />);

    expect(
      await screen.findByText("https://acme.feedbackmonk.com"),
    ).toBeInTheDocument();
    // The CNAME target is the ONE string a customer copies into their DNS
    // panel; if it is wrong or missing the whole paid feature is unusable.
    const cname = screen.getByText("acme.feedbackmonk.com");
    expect(cname.tagName).toBe("CODE");
  });

  it("saves a changed subdomain, and keeps Save disabled while it is unchanged", async () => {
    mockedFetch.mockResolvedValueOnce(hosting());
    mockedPutSubdomain.mockResolvedValueOnce(
      hosting({ subdomain: "acme-corp", public_host: "acme-corp.feedbackmonk.com" }),
    );
    const user = userEvent.setup();
    renderWithClient(<HostingSettings />);

    const input = await screen.findByLabelText("Your subdomain");
    const save = screen.getByRole("button", { name: "Save" });
    // Changing your public address breaks shared links, so it must be a
    // deliberate act — never a stray keystroke on a live-saving field.
    expect(save).toBeDisabled();

    await user.clear(input);
    await user.type(input, "acme-corp");
    expect(save).toBeEnabled();
    await user.click(save);

    expect(mockedPutSubdomain).toHaveBeenCalledWith("acme-corp");
  });

  it("clears the subdomain when the field is emptied", async () => {
    mockedFetch.mockResolvedValueOnce(hosting());
    mockedPutSubdomain.mockResolvedValueOnce(
      hosting({ subdomain: null, public_host: null }),
    );
    const user = userEvent.setup();
    renderWithClient(<HostingSettings />);

    const input = await screen.findByLabelText("Your subdomain");
    await user.clear(input);
    await user.click(screen.getByRole("button", { name: "Save" }));

    // An empty string must become an explicit null, not the literal "".
    expect(mockedPutSubdomain).toHaveBeenCalledWith(null);
  });

  it("offers the claim form on a tier that carries custom_domain", async () => {
    mockedFetch.mockResolvedValueOnce(hosting({ tier: "pro" }));
    mockedClaim.mockResolvedValueOnce({
      id: "d1",
      domain: "feedback.acme.example",
      status: "pending",
      created_at: "2026-08-30T00:00:00Z",
      verified_at: null,
    });
    const user = userEvent.setup();
    renderWithClient(<HostingSettings />);

    const input = await screen.findByLabelText("Hostname");
    await user.type(input, "feedback.acme.example");
    await user.click(screen.getByRole("button", { name: "Add domain" }));

    expect(mockedClaim).toHaveBeenCalledWith("feedback.acme.example");
  });

  it("shows an upgrade prompt instead of the claim form on a tier without custom_domain", async () => {
    mockedFetch.mockResolvedValueOnce(
      hosting({ tier: "free", custom_domain_available: false }),
    );
    renderWithClient(<HostingSettings />);

    expect(await screen.findByText(/Your own domain/)).toBeInTheDocument();
    expect(
      screen.getByText(/Custom domains are available on the Pro plan/),
    ).toBeInTheDocument();
    // The form must be absent, not merely disabled — but note this is UX, not
    // the gate: the server returns 402 at claim time and refuses the
    // certificate at issuance time regardless of what this component renders.
    expect(screen.queryByLabelText("Hostname")).not.toBeInTheDocument();
  });

  it("renders domain status as words, not colour alone (WCAG 1.4.1)", async () => {
    mockedFetch.mockResolvedValueOnce(
      hosting({
        domains: [
          {
            id: "d1",
            domain: "live.acme.example",
            status: "active",
            created_at: "2026-08-30T00:00:00Z",
            verified_at: "2026-08-30T01:00:00Z",
          },
          {
            id: "d2",
            domain: "waiting.acme.example",
            status: "pending",
            created_at: "2026-08-30T00:00:00Z",
            verified_at: null,
          },
        ],
      }),
    );
    renderWithClient(<HostingSettings />);

    expect(await screen.findByText("Live")).toBeInTheDocument();
    expect(screen.getByText("Waiting for DNS")).toBeInTheDocument();
    // Each Remove button names its own domain for screen-reader users, so a
    // list of them is not five identical "Remove" controls.
    expect(
      screen.getByRole("button", { name: /Remove live\.acme\.example/ }),
    ).toBeInTheDocument();
  });

  it("says plainly that subdomains are unavailable on a deployment without a root domain", async () => {
    // The self-host posture (FEEDBACKMONK_ROOT_DOMAIN unset). Showing an empty
    // field that silently produces no address would be worse than saying so.
    mockedFetch.mockResolvedValueOnce(
      hosting({
        tier: "self_host",
        subdomain: null,
        public_host: null,
        cname_target: null,
        custom_domain_available: true,
      }),
    );
    renderWithClient(<HostingSettings />);

    expect(
      await screen.findByText(/doesn’t use tenant subdomains/),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Your subdomain")).not.toBeInTheDocument();
    // And the custom-domain section is hidden too: without a CNAME target
    // there are no instructions to give.
    expect(screen.queryByText("Your own domain")).not.toBeInTheDocument();
  });
});
