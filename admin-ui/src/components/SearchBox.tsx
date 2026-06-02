import { useEffect, useId, useRef, useState } from "react";

export const SEARCH_DEBOUNCE_MS = 250;

interface SearchBoxProps {
  /** The committed query (e.g. mirrored from the URL `q` param). */
  value: string;
  /** Called with the trimmed query after the debounce interval settles. */
  onSearch: (query: string) => void;
  /** Debounce interval; defaults to {@link SEARCH_DEBOUNCE_MS}. */
  delayMs?: number;
  label?: string;
  placeholder?: string;
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
  label = "Search feedback",
  placeholder = "Search feedback…",
}: SearchBoxProps) {
  const fieldId = useId();
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

  function clear() {
    setText("");
    lastCommitted.current = "";
    onSearch("");
  }

  return (
    <div className="search-box" role="search">
      <label htmlFor={fieldId}>{label}</label>
      <input
        id={fieldId}
        type="search"
        value={text}
        placeholder={placeholder}
        autoComplete="off"
        onChange={(e) => setText(e.target.value)}
      />
      {text ? (
        <button type="button" className="search-clear" onClick={clear}>
          Clear search
        </button>
      ) : null}
    </div>
  );
}
