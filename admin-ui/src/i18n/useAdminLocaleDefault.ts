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
// 2. IT IS A DEFAULT, NOT AN OVERRIDE. It applies only when the visitor has
//    made no choice of their own — no `?lang=` on this view, nothing in
//    `fbm_lang`. An owner who set the tenant to German and then picked English
//    in the switcher stays in English.

import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { getLocaleSettings } from "../shared/localeApi";
import { readQueryLocale, readStoredLocale, setLocale } from "./useLocale";

export function isAdminPath(pathname: string): boolean {
  return pathname === "/admin" || pathname.startsWith("/admin/");
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
