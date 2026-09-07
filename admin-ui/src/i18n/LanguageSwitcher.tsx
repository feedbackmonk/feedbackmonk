import { useId, useState } from "react";
import { LOCALES } from "./locales.gen";
import { useLocale } from "./useLocale";
import { useTranslation } from "./index";

// The language control on every public surface (FR-FBR-36).
//
// A NATIVE `<select>`, deliberately: it is the one listbox that already works
// with every screen reader, every mobile OS picker and keyboard-only use, in
// 31 languages, with no ARIA of our own to get wrong.
//
// Each `<option>` carries its own `lang` so a screen reader pronounces
// "Français" in French rather than reading it in the page language, and the
// visible label is the ENDONYM (the language's own name for itself) — a
// visitor looking for their language does not know what we call it in English.
//
// CHANGING LANGUAGE NEVER NAVIGATES. No redirect to a `/de/` path, no
// `?lang=` rewrite: the URL a visitor shared stays the URL they shared.

export function LanguageSwitcher({ className }: { className?: string }) {
  const { locale, setLocale } = useLocale();
  const { t } = useTranslation("public");
  const id = useId();
  // R-A11Y A-4: a runtime language change swaps the whole page's text with no
  // announcement. `setLocale` is awaited so `t` below is already bound to the
  // NEW language by the time this renders the confirmation — a screen-reader
  // user hears the change confirmed in the language it changed TO, not the one
  // it changed FROM.
  const [announcement, setAnnouncement] = useState("");

  async function onChange(code: string) {
    await setLocale(code);
    const name = LOCALES.find((l) => l.code === code)?.name ?? code;
    setAnnouncement(t("public.switcher.changed", { name }));
  }

  return (
    <div className={className ?? "language-switcher"}>
      <select
        id={id}
        className="language-switcher-select"
        aria-label={t("public.switcher.label")}
        value={locale}
        onChange={(e) => void onChange(e.target.value)}
      >
        {LOCALES.map((l) => (
          <option key={l.code} value={l.code} lang={l.code}>
            {l.name}
          </option>
        ))}
      </select>
      <span role="status" className="visually-hidden">
        {announcement}
      </span>
    </div>
  );
}
