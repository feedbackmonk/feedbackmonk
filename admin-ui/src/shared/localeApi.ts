// admin-ui/src/shared/localeApi.ts
//
// Admin-UI client + type mirror for the tenant language setting (Contract C38,
// FR-FBR-38). Owner of the wire shape is the backend
// (`crates/feedbackmonk-api/src/handlers/tenant_settings.rs`); this file is the
// single reconcile point on the client.
//
// WHY A SEPARATE FILE (not shared/ApiClient.ts): same reasoning as
// `hostingApi.ts` / `boardModerationApi.ts` — a self-contained admin surface
// with its own frozen contract keeps its paths and shapes pinned in one place.
// It reuses the shared axios `api` instance, so baseURL, credentials and the
// 401 interceptor all still apply.
//
// ⚠ THE 401 INTERCEPTOR IS WHY THIS IS ADMIN-ONLY. `api`'s response
// interceptor redirects to `/login` on 401, so calling these from a PUBLIC
// board/roadmap view would bounce an anonymous visitor into the admin login
// page. `useAdminLocaleDefault` gates the read on an `/admin` path for exactly
// that reason — do not lift it.
//
// Contract C38:
//   GET  /api/v1/admin/settings/locale → 200 { locale: string|null, translate_outbound: boolean }
//   PUT  /api/v1/admin/settings/locale   (both fields optional) → 200 echo
//   unknown locale code                  → 400 { code: "invalid_locale" }

import { api } from "./ApiClient";

const LOCALE_PATH = "/admin/settings/locale";

export interface LocaleSettings {
  /**
   * The tenant's chosen UI locale — a C34 code — or `null` for "follow the
   * browser". `null` is a real, chosen state, not a missing value.
   */
  locale: string | null;
  /**
   * FR-FBR-40 — outbound reply translation. When on, the server machine-
   * translates the team's own status notes and public replies into the
   * submitter's language before emailing them (the original is always included
   * below the translation). Off by default, and inert unless the deployment
   * also configured a translation provider, which is itself off by default —
   * so ticking this on a deployment with no provider changes nothing.
   */
  translate_outbound: boolean;
}

export interface LocaleSettingsPatch {
  locale?: string | null;
  translate_outbound?: boolean;
}

/** The 400 body C38 returns for a code outside the shipped table. */
export interface InvalidLocaleBody {
  code: "invalid_locale";
}

export function isInvalidLocale(body: unknown): body is InvalidLocaleBody {
  return (
    typeof body === "object" &&
    body !== null &&
    (body as { code?: unknown }).code === "invalid_locale"
  );
}

export async function getLocaleSettings(): Promise<LocaleSettings> {
  const r = await api.get<LocaleSettings>(LOCALE_PATH);
  return r.data;
}

export async function putLocaleSettings(
  patch: LocaleSettingsPatch,
): Promise<LocaleSettings> {
  const r = await api.put<LocaleSettings>(LOCALE_PATH, patch);
  return r.data;
}
