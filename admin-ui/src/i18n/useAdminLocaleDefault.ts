// The tenant's configured locale as a DEFAULT for admin surfaces (Contract
// C38, precedence rung 3 in `useLocale`).
//
// Two constraints shape this hook:
//
// 1. ADMIN PATHS ONLY. `shared/ApiClient`'s 401 interceptor redirects to
//    `/login`, so issuing this request from a public board/roadmap view would
//    bounce an anonymous visitor out of a page they are entitled to read. The
//    tenant default is an ADMIN convenience; a public visitor's language comes
//    from their own browser and their own choice.
//
//    "Admin path" is the authenticated console, which is `/admin/*` AND
//    `/feedback[/FB-XXXXXX]` — the console's busiest screen, which is not under
//    `/admin` only for historical URL reasons (App.tsx's route table). It
//    already issues `GET /api/v1/admin/feedback` through the same axios
//    instance and already 401-redirects on its own, so the interceptor concern
//    above does not apply to it. Excluding it made the console's `lang` AND
//    `dir` flip on navigation to any `/admin/*` page and revert on reload,
//    within one tab (R-A11Y finding A-1, demonstrated end to end).
//    `/login` stays out (unauthenticated), as do the public surfaces.
//
// 2. IT IS A DEFAULT, NOT AN OVERRIDE. It applies only when the visitor has
//    made no choice of their own — no `?lang=` on this view, nothing in
//    `fbm_lang`. An owner who set the tenant to German and then picked English
//    in the switcher stays in English.

import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { getLocaleSettings } from "../shared/localeApi";
import { readQueryLocale, readStoredLocale, setLocale } from "./useLocale";

export function isAdminPath(pathname: string): boolean {
  return (
    pathname === "/admin" ||
    pathname.startsWith("/admin/") ||
    pathname === "/feedback" ||
    pathname.startsWith("/feedback/")
  );
}

/** True when nothing the visitor chose is already deciding the language. */
export function wantsTenantDefault(): boolean {
  return readQueryLocale() === null && readStoredLocale() === null;
}

export function useAdminLocaleDefault(pathname: string): void {
  const enabled = isAdminPath(pathname) && wantsTenantDefault();

  const query = useQuery({
    queryKey: ["admin-locale-settings"],
    queryFn: getLocaleSettings,
    enabled,
    // A tenant's language does not change under a page view, and a failure
    // here is a non-event: the browser-resolved locale already applies.
    staleTime: Infinity,
    retry: false,
  });

  const tenantLocale = query.data?.locale ?? null;

  useEffect(() => {
    if (!enabled || !tenantLocale) return;
    // `persist: false` — this is the tenant's default, not the visitor's
    // choice; it must not be written into `fbm_lang`.
    void setLocale(tenantLocale, { persist: false });
  }, [enabled, tenantLocale]);
}
