import { useEffect, useId, useRef, useState } from "react";
import { Trans } from "react-i18next";
import { useTranslation } from "../i18n";

export const SEARCH_DEBOUNCE_MS = 250;
/** Default single-key shortcut that focuses the search field from anywhere on the page. */
export const SEARCH_FOCUS_KEY = "/";

interface SearchBoxProps {
  /** The committed query (e.g. mirrored from the URL `q` param). */
  value: string;
  /** Called with the trimmed query after the debounce interval settles. */
  onSearch: (query: string) => void;
  /** Debounce interval; defaults to {@link SEARCH_DEBOUNCE_MS}. */
  delayMs?: number;
  label?: string;
  placeholder?: string;
  /**
   * Page-wide key that focuses the field when pressed outside any editable
   * element; defaults to {@link SEARCH_FOCUS_KEY}. Pass `null` to disable.
   */
  focusKey?: string | null;
}

// True when a keypress originated in something that consumes typing, so a
// page-wide shortcut must not steal it (typing "/" in a reply, say).
function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";
}

// Debounced full-text search box for the admin feedback list (parity gap #3).
// Debounces client-side so we never fire a request per keystroke (task brief:
// ~250ms). The committed `value` is the source of truth (URL-backed); local
// `text` tracks in-flight typing and is reconciled when `value` changes
// externally (e.g. back/forward navigation or a cleared filter).
export function SearchBox({
  value,
  onSearch,
  delayMs = SEARCH_DEBOUNCE_MS,
  label,
  placeholder,
  focusKey = SEARCH_FOCUS_KEY,
}: SearchBoxProps) {
  const { t } = useTranslation("admin");
  const resolvedLabel = label ?? t("admin.searchBox.label");
  const resolvedPlaceholder = placeholder ?? t("admin.searchBox.placeholder");
  const fieldId = useId();
  const hintId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const [text, setText] = useState(value);
  // Track the last value we committed so an external `value` change (not
  // caused by our own debounce) re-syncs the input without clobbering typing.
  const lastCommitted = useRef(value);

  useEffect(() => {
    if (value !== lastCommitted.current) {
      lastCommitted.current = value;
      setText(value);
    }
  }, [value]);

  useEffect(() => {
    const trimmed = text.trim();
    if (trimmed === lastCommitted.current.trim()) return;
    const handle = setTimeout(() => {
      lastCommitted.current = trimmed;
      onSearch(trimmed);
    }, delayMs);
    return () => clearTimeout(handle);
  }, [text, delayMs, onSearch]);

  // Page-wide focus shortcut. Ignores modified keys and keypresses that
  // originate inside another editable control.
  useEffect(() => {
    if (!focusKey) return;
    function onKeyDown(e: KeyboardEvent) {
      if (e.key !== focusKey) return;
      if (e.ctrlKey || e.metaKey || e.altKey || e.defaultPrevented) return;
      if (isEditableTarget(e.target)) return;
      e.preventDefault();
      inputRef.current?.focus();
      inputRef.current?.select();
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [focusKey]);

  function clear() {
    setText("");
    lastCommitted.current = "";
    onSearch("");
  }

  // Layout: the label and the Clear control share one fixed-height row above
  // the field, so Clear appearing/disappearing never shifts anything below.
  // Clear stays a real <button> (keyboard + AT semantics) styled as a link.
  // The syntax hint is always rendered for the same reason (stable height) and
  // doubles as the field's accessible description.
  return (
    <div className="search-box" role="search">
      <div className="search-box-head">
        <label htmlFor={fieldId}>{resolvedLabel}</label>
        {text ? (
          <button
            type="button"
            className="link-button search-clear"
            onClick={clear}
          >
            {t("admin.searchBox.clear")}
          </button>
        ) : null}
      </div>
      <div className="search-field">
        <svg
          className="search-icon"
          aria-hidden="true"
          focusable="false"
          viewBox="0 0 24 24"
        >
          <circle cx="11" cy="11" r="7" />
          <path d="m20 20-3.6-3.6" />
        </svg>
        <input
          ref={inputRef}
          id={fieldId}
          type="search"
          value={text}
          placeholder={resolvedPlaceholder}
          autoComplete="off"
          aria-describedby={hintId}
          aria-keyshortcuts={focusKey ?? undefined}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape" && text) {
              e.preventDefault();
              clear();
            }
          }}
        />
        {focusKey ? (
          <kbd className="search-kbd" aria-hidden="true">
            {focusKey}
          </kbd>
        ) : null}
      </div>
      <p id={hintId} className="search-hint">
        <Trans
          i18nKey="admin.searchBox.tips"
          t={t}
          components={{
            code1: <code />,
            code2: <code />,
            code3: <code />,
          }}
        />
        {focusKey ? (
          <Trans
            i18nKey="admin.searchBox.tipsShortcut"
            t={t}
            values={{ key: focusKey }}
            components={{ kbd: <kbd /> }}
          />
        ) : null}
      </p>
    </div>
  );
}
