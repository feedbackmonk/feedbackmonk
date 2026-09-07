import { useId } from "react";
import axios from "axios";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  getLocaleSettings,
  isInvalidLocale,
  putLocaleSettings,
  type LocaleSettings as LocaleSettingsShape,
} from "../../shared/localeApi";
import { useToast } from "../../components/Toast";
import { LOCALES } from "../../i18n/locales.gen";
import {
  browserLocale,
  clearStoredLocale,
  setLocale,
  useLocale,
} from "../../i18n/useLocale";

// /admin/settings/language — the tenant's default UI language (FR-FBR-38,
// Contract C38). Query/mutation chrome mirrors BoardSettings.
//
// WHAT THIS SETTING IS: the language this tenant's ADMIN surfaces open in, and
// the language feedbackmonk writes the tenant's own emails in when it has
// nothing better to go on. It is a DEFAULT — a visitor's own choice (the
// public switcher, `?lang=`) always wins for that visitor, and an end-user's
// submitted locale wins for the emails they receive (FR-FBR-37).
//
// "Browser default" (`null`) is a real choice, not an empty field: it means
// "follow whatever language this browser asks for", and selecting it forgets
// the choice stored on this device.
//
// [CLAUDE-B][NOTE] Stage 2 / W-D: this page's own strings are deliberately NOT
// extracted here — `i18n/locales/*/admin.json` is W-D's file (plan § W-D), and
// splitting a namespace across two workers is what the per-namespace catalog
// layout exists to prevent. The keys belong under `admin.settings.language.*`.
export function LanguageSettings() {
  const selectId = useId();
  const queryClient = useQueryClient();
  const { notify } = useToast();
  const { locale: activeLocale } = useLocale();
  const queryKey = ["admin-locale-settings"];

  const query = useQuery({ queryKey, queryFn: getLocaleSettings });

  const mutation = useMutation({
    mutationFn: (next: string | null) => putLocaleSettings({ locale: next }),
    onSuccess: (data) => {
      queryClient.setQueryData<LocaleSettingsShape>(queryKey, data);
      // Apply immediately — a language setting that needs a reload to be seen
      // is a language setting nobody trusts.
      if (data.locale) {
        void setLocale(data.locale);
      } else {
        clearStoredLocale();
        void setLocale(browserLocale(), { persist: false });
      }
      notify("Language saved.", "success");
    },
    onError: (err) => {
      const body = axios.isAxiosError(err) ? err.response?.data : undefined;
      notify(
        isInvalidLocale(body)
          ? "That language isn’t one feedbackmonk ships."
          : "Could not save the language setting.",
        "error",
      );
    },
  });

  const settings = query.data;
  const saving = mutation.isPending;

  return (
    <main
      className="language-settings-page"
      aria-labelledby="language-settings-title"
    >
      <header className="page-header">
        <h1 id="language-settings-title">Language</h1>
        <p className="muted">
          The language your admin pages open in, and the language we write your
          emails in when we have nothing else to go on. People reading your
          public board always choose their own.
        </p>
      </header>

      {query.isError ? (
        <div role="alert" className="error-block">
          Failed to load the language setting.{" "}
          <button type="button" onClick={() => query.refetch()}>
            Retry
          </button>
        </div>
      ) : null}

      {query.isPending ? (
        <p className="muted" aria-busy="true">
          Loading…
        </p>
      ) : settings ? (
        <section className="language-settings-card" aria-label="Language">
          <label htmlFor={selectId}>Default language</label>
          <select
            id={selectId}
            value={settings.locale ?? ""}
            disabled={saving}
            aria-describedby={`${selectId}-note`}
            onChange={(e) =>
              mutation.mutate(e.target.value === "" ? null : e.target.value)
            }
          >
            <option value="">Browser default</option>
            {LOCALES.map((l) => (
              <option key={l.code} value={l.code} lang={l.code}>
                {l.name}
              </option>
            ))}
          </select>
          <p id={`${selectId}-note`} className="muted">
            {settings.locale
              ? "Everyone in this workspace sees the admin in this language unless they pick another one."
              : `Following this browser — currently ${activeLocale}.`}
          </p>
        </section>
      ) : null}
    </main>
  );
}
