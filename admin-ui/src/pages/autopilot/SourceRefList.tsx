import type { SourceRef } from "../../shared/types.gen";
import { useTranslation } from "../../i18n";

// Renders a recommendation's `source_refs` as CITATIONS — the file/line/doc
// references the analyst inspected — NEVER as content dumps. This is an
// exfiltration defense (C24 case f): the grounding evidence is a pointer, not
// the pointed-at bytes. Every field is untrusted jsonb from the analyst seam,
// so it is rendered as escaped text only and the shape is treated defensively
// (a malformed entry degrades to its stringified form, never crashes).
export function SourceRefList({ refs }: { refs: SourceRef[] }) {
  const { t } = useTranslation("admin");
  if (!Array.isArray(refs) || refs.length === 0) {
    return (
      <p className="muted ap-source-empty">
        {t("admin.sourceRefList.empty")}
      </p>
    );
  }
  return (
    <ul className="ap-source-refs">
      {refs.map((ref, i) => (
        <li key={i} className="ap-source-ref">
          <code className="mono">{citationFor(ref, t)}</code>
          {ref && typeof ref === "object" && ref.detail ? (
            <span className="muted"> — {String(ref.detail)}</span>
          ) : null}
        </li>
      ))}
    </ul>
  );
}

// Prefer an explicit label, then a ref/locator, then a defensive stringify.
// Returns a short citation string ONLY — deliberately never echoes a `detail`
// or any large field that could carry smuggled content.
function citationFor(
  ref: SourceRef,
  t: (key: string) => string,
): string {
  const fallback = t("admin.sourceRefList.unlabeled");
  if (!ref || typeof ref !== "object") return String(ref ?? fallback);
  if (typeof ref.label === "string" && ref.label) return ref.label;
  if (typeof ref.ref === "string" && ref.ref) {
    return ref.kind ? `${ref.kind}: ${ref.ref}` : ref.ref;
  }
  return fallback;
}
