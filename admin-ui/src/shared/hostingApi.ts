// admin-ui/src/shared/hostingApi.ts
//
// Admin-UI client + type mirror for the hosting surface (Contract C33,
// FR-FBR-32/33). Tenant subdomain + custom-domain claim/release.
//
// WHY A SEPARATE FILE (not shared/ApiClient.ts): same reasoning as
// `boardModerationApi.ts` — a self-contained admin surface with its own frozen
// contract keeps its paths and shapes pinned in one place. It reuses the shared
// axios `api` instance, so baseURL, credentials and the 401 interceptor all
// still apply.
//
// SEAM ISOLATION: every path and response shape is pinned here, against the
// Rust handlers in `crates/feedbackmonk-api/src/handlers/domains.rs`. This file
// is the single reconcile point if those move.

import { api } from "./ApiClient";
import type { Tier } from "./types.gen";

// ─────────────────────────────────────────────────────────────────────────
// Paths (Contract C33)
// ─────────────────────────────────────────────────────────────────────────

const HOSTING_PATHS = {
  hosting: "/admin/hosting",
  subdomain: "/admin/hosting/subdomain",
  domains: "/admin/hosting/domains",
  domain: (id: string) => `/admin/hosting/domains/${encodeURIComponent(id)}`,
} as const;

// ─────────────────────────────────────────────────────────────────────────
// Wire shapes
// ─────────────────────────────────────────────────────────────────────────

/** Mirror of `tenant_domains.status` (migration 00030). */
export type DomainStatus = "pending" | "active";

export interface TenantDomain {
  id: string;
  domain: string;
  status: DomainStatus;
  created_at: string;
  verified_at: string | null;
}

export interface HostingSettings {
  /** Current pricing tier — drives the shared UpgradePrompt copy. */
  tier: Tier;
  /** The tenant's DNS label, or null when they have not chosen one. */
  subdomain: string | null;
  /** `{subdomain}.{root_domain}`, or null if either half is missing. */
  public_host: string | null;
  /**
   * What the customer puts on the right-hand side of their CNAME record.
   * Null when the deployment offers no subdomains — in which case it offers no
   * custom domains either, and the whole section is hidden.
   */
  cname_target: string | null;
  /**
   * Whether this tenant's tier includes custom domains. Advisory ONLY: the
   * server enforces independently at claim time (402) and again at certificate
   * issuance. This flag decides whether the UI shows a form or an upgrade
   * prompt — it is not the gate.
   */
  custom_domain_available: boolean;
  domains: TenantDomain[];
}

// ─────────────────────────────────────────────────────────────────────────
// Calls
// ─────────────────────────────────────────────────────────────────────────

export async function fetchHosting(): Promise<HostingSettings> {
  const r = await api.get<HostingSettings>(HOSTING_PATHS.hosting);
  return r.data;
}

/** Set the tenant subdomain, or clear it by passing `null`. */
export async function putSubdomain(
  subdomain: string | null,
): Promise<HostingSettings> {
  const r = await api.put<HostingSettings>(HOSTING_PATHS.subdomain, {
    subdomain,
  });
  return r.data;
}

export async function claimDomain(domain: string): Promise<TenantDomain> {
  const r = await api.post<TenantDomain>(HOSTING_PATHS.domains, { domain });
  return r.data;
}

export async function releaseDomain(id: string): Promise<void> {
  await api.delete(HOSTING_PATHS.domain(id));
}

// ─────────────────────────────────────────────────────────────────────────
// Public host discovery (Contract C32) — unauthenticated.
//
// `GET /api/v1/public/site` answers "which tenant and projects does THIS host
// serve". It is how a tenant subdomain renders a board without a project id in
// the URL, and it is deliberately incapable of enumeration: it only ever
// describes the tenant whose host the caller already reached.
// ─────────────────────────────────────────────────────────────────────────

export interface SiteProject {
  project_id: string;
  slug: string;
  name: string;
  public_board_enabled: boolean;
}

export interface PublicSite {
  tenant_id: string;
  subdomain: string | null;
  /** True when the visitor arrived on the tenant's own domain, not `{sub}.{root}`. */
  custom_domain: boolean;
  projects: SiteProject[];
}

/**
 * Describe the current host.
 *
 * Returns `null` for a host that is not tenant-bound — the admin host, an
 * unknown name, or any host at all on a deployment with no root domain
 * configured. That is a ROUTING answer, not an error: it is the normal case
 * everywhere except a tenant subdomain or a claimed custom domain.
 */
export async function fetchPublicSite(): Promise<PublicSite | null> {
  try {
    const r = await api.get<PublicSite>("/public/site");
    return r.data;
  } catch (err: unknown) {
    if ((err as { response?: { status?: number } })?.response?.status === 404) {
      return null;
    }
    throw err;
  }
}
