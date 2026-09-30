import { useQuery } from "@tanstack/react-query";
import { Trans } from "react-i18next";
import { fetchFeedbackCrash } from "../shared/ApiClient";
import { formatAbsolute } from "../shared/format";
import { useTranslation } from "../i18n";
import { useLocale } from "../i18n/useLocale";

// Only an http(s) permalink becomes a link: the URL comes from a third-party
// tracker, and a `javascript:` or `data:` value must never be clickable.
export function safeHttpUrl(url: string | null | undefined): string | null {
  if (!url) return null;
  try {
    const u = new URL(url);
    return u.protocol === "https:" || u.protocol === "http:" ? u.toString() : null;
  } catch {
    return null;
  }
}

// Parity gap #2 (adoption contract §5.6): the crash this feedback was linked
// to at submit time, resolved from the crash tracker. A separate, best-effort
// request -- a slow or down tracker delays only this banner, never the drawer,
// and every failure degrades to "details unavailable" with the id still shown.
// Tracker text is rendered as plain text, never as HTML.
export function CrashBanner({
  feedbackId,
  crashEventId,
}: {
  feedbackId: string;
  crashEventId: string;
}) {
  const { t } = useTranslation("admin");
  const { locale } = useLocale();
  const query = useQuery({
    queryKey: ["admin-feedback-crash", feedbackId],
    queryFn: () => fetchFeedbackCrash(feedbackId),
    retry: false,
  });
  const data = query.data;
  if (data?.status === "none") return null;

  let content;
  if (query.isPending) {
    content = <p className="muted">{t("admin.feedbackDrawer.crashLoading")}</p>;
  } else if (data?.status === "linked" && data.crash) {
    const crash = data.crash;
    const href = safeHttpUrl(crash.permalink);
    content = (
      <>
        <p className="crash-title">{crash.title}</p>
        {crash.culprit ? <p className="mono">{crash.culprit}</p> : null}
        <p className="muted">
          {crash.level ? t("admin.feedbackDrawer.crashLevel", { level: crash.level }) : null}
          {crash.level && crash.last_seen ? " · " : null}
          {crash.last_seen ? (
            <Trans
              i18nKey="admin.feedbackDrawer.crashLastSeen"
              t={t}
              values={{ when: formatAbsolute(crash.last_seen, locale) }}
              components={{ time: <time dateTime={crash.last_seen} /> }}
            />
          ) : null}
        </p>
        {href ? (
          <a href={href} target="_blank" rel="noopener noreferrer">
            {t("admin.feedbackDrawer.crashOpen")}
          </a>
        ) : null}
      </>
    );
  } else {
    const id = data?.crash_event_id ?? crashEventId;
    content = (
      <p className="muted">
        {data?.status === "not_found"
          ? t("admin.feedbackDrawer.crashNotFound", { id })
          : t("admin.feedbackDrawer.crashUnavailable", { id })}
      </p>
    );
  }

  return (
    <section aria-labelledby="drawer-crash-label" className="crash-banner">
      <h3 id="drawer-crash-label">{t("admin.feedbackDrawer.crashHeading")}</h3>
      {content}
    </section>
  );
}
