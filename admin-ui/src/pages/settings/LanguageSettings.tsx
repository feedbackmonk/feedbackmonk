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
import { LOCALES, localeByCode } from "../../i18n/locales.gen";
import {
  browserLocale,
  clearStoredLocale,
  setLocale,
  useLocale,
} from "../../i18n/useLocale";
import { useTranslation } from "../../i18n";

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
export function LanguageSettings() {
  const { t } = useTranslation("admin");
  const selectId = useId();
  const outboundId = useId();
  const queryClient = useQueryClient();
  const { notify } = useToast();
  const { locale: activeLocale } = useLocale();
  const queryKey = ["admin-locale-settings"];

  const query = useQuery({ queryKey, queryFn: getLocaleSettings });

  const mutation = useMutation({
    mutationFn: (next: string | null) => putLocaleSettings({ locale: next }),
    onSuccess: async (data) => {
      queryClient.setQueryData<LocaleSettingsShape>(queryKey, data);
      // Apply immediately — a language setting that needs a reload to be seen
      // is a language setting nobody trusts. AWAITED (R-A11Y A-4): the toast
      // below must resolve its text with i18next already on the new language,
      // not the old one — `<html lang>` and the toast copy must agree.
      if (data.locale) {
        await setLocale(data.locale);
      } else {
        clearStoredLocale();
        await setLocale(browserLocale(), { persist: false });
      }
      notify(t("admin.settings.language.saved"), "success");
    },
    onError: (err) => {
      const body = axios.isAxiosError(err) ? err.response?.data : undefined;
      notify(
        isInvalidLocale(body)
          ? t("admin.settings.language.errors.invalidLocale")
          : t("admin.settings.language.errors.saveFailed"),
        "error",
      );
    },
  });

  // A checkbox-only PUT must not send `locale` — a separate mutation from the
  // one above, since C38's absent-vs-null rule means an absent field leaves
  // the stored locale alone (CLAUDE-E hand-over, GUIDE § 7.2).
  const outboundMutation = useMutation({
    mutationFn: (next: boolean) => putLocaleSettings({ translate_outbound: next }),
    onSuccess: (data) => {
      queryClient.setQueryData<LocaleSettingsShape>(queryKey, data);
      notify(t("admin.settings.language.saved"), "success");
    },
    onError: () => notify(t("admin.settings.language.errors.saveFailed"), "error"),
  });

  const settings = query.data;
  const saving = mutation.isPending;

  return (
    <main
      className="language-settings-page"
      aria-labelledby="language-settings-title"
    >
      <header className="page-header">
        <h1 id="language-settings-title">{t("admin.settings.language.title")}</h1>
        <p className="muted">{t("admin.settings.language.intro")}</p>
      </header>

      {query.isError ? (
        <div role="alert" className="error-block">
          {t("admin.settings.language.loadError")}{" "}
          <button type="button" onClick={() => query.refetch()}>
            {t("admin.common.retry")}
          </button>
        </div>
      ) : null}

      {query.isPending ? (
        <p className="muted" aria-busy="true">
          {t("admin.common.loading")}
        </p>
      ) : settings ? (
        <section
          className="language-settings-card"
          aria-label={t("admin.settings.language.title")}
        >
          <label htmlFor={selectId}>
            {t("admin.settings.language.defaultLanguageLabel")}
          </label>
          <select
            id={selectId}
            value={settings.locale ?? ""}
            disabled={saving}
            aria-describedby={`${selectId}-note`}
            onChange={(e) =>
              mutation.mutate(e.target.value === "" ? null : e.target.value)
            }
          >
            <option value="">
              {t("admin.settings.language.browserDefault")}
            </option>
            {LOCALES.map((l) => (
              <option key={l.code} value={l.code} lang={l.code}>
                {l.name}
              </option>
            ))}
          </select>
          <p id={`${selectId}-note`} className="muted">
            {settings.locale
              ? t("admin.settings.language.workspaceNote")
              : t("admin.settings.language.followingBrowser", {
                  // The endonym ("Deutsch"), not the raw BCP-47 tag ("de") —
                  // it's what the <select> above already displays, and what an
                  // AT should say instead of a code (R-A11Y A-6).
                  locale: localeByCode(activeLocale)?.name ?? activeLocale,
                })}
          </p>

          <div className="language-settings-outbound">
            <input
              type="checkbox"
              id={outboundId}
              checked={settings.translate_outbound}
              disabled={outboundMutation.isPending}
              aria-describedby={`${outboundId}-note`}
              onChange={(e) => outboundMutation.mutate(e.target.checked)}
            />
            <label htmlFor={outboundId}>
              {t("admin.settings.language.translateOutbound.label")}
            </label>
            <p id={`${outboundId}-note`} className="muted">
              {t("admin.settings.language.translateOutbound.description")}
            </p>
          </div>
        </section>
      ) : null}
    </main>
  );
}
