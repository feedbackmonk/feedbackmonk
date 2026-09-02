import type { ReactNode } from "react";

// Client-side match highlighting for full-text search hits.
//
// The backend ranks rows with Postgres `websearch_to_tsquery` but returns a
// plain 200-char excerpt with no hit offsets, so the list would otherwise show
// WHAT matched without showing WHY. This module derives the highlightable terms
// from the query the user typed and wraps their occurrences in <mark>. It is a
// best-effort visual aid, never the source of truth for what matched.

// Words `websearch_to_tsquery` treats as an operator or that the `english`
// configuration drops as a stopword. Highlighting them would only add noise.
const SKIP_TOKENS = new Set([
  "or",
  "and",
  "not",
  "the",
  "a",
  "an",
  "of",
  "to",
  "in",
  "is",
  "it",
  "on",
  "for",
]);

// Postgres stems both the query and the body ("buttons" ⇢ "button"), so a hit
// may carry a different inflection than the one typed. Trimming common English
// suffixes from the typed term and matching what remains as a substring keeps
// "buttons" lighting up "button" and "crashed" lighting up "crashes".
function stem(word: string): string {
  if (word.length <= 4) return word;
  if (/ies$/i.test(word)) return word.slice(0, -3) + "y";
  return word.replace(/(ing|ed|es|s)$/i, "");
}

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Terms worth highlighting for a `websearch_to_tsquery`-style query.
 *
 * - `"quoted phrases"` are kept whole.
 * - `-excluded` tokens are dropped (a hit cannot contain them).
 * - operator words and stopwords are dropped.
 * - bare words are lightly stemmed (see {@link stem}).
 *
 * Returned longest-first and de-duplicated so alternation in a regex prefers
 * the longer term when one is a prefix of another.
 */
export function extractSearchTerms(query: string): string[] {
  const terms: string[] = [];
  const re = /"([^"]*)"|(\S+)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(query)) !== null) {
    if (m[1] !== undefined) {
      const phrase = m[1].trim();
      if (phrase.length >= 2) terms.push(phrase.toLowerCase());
      continue;
    }
    const token = m[2];
    if (token.startsWith("-")) continue;
    const clean = token.replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, "");
    if (clean.length < 2) continue;
    if (SKIP_TOKENS.has(clean.toLowerCase())) continue;
    terms.push(stem(clean).toLowerCase());
  }
  return [...new Set(terms)].sort((a, b) => b.length - a.length);
}

/**
 * Wrap every case-insensitive occurrence of the query's terms in
 * `<mark class="search-mark">`. Returns the input string untouched when the
 * query yields no highlightable terms or nothing matches, so callers can pass
 * the result straight into JSX either way.
 */
export function highlightMatches(text: string, query: string): ReactNode {
  const terms = extractSearchTerms(query);
  if (terms.length === 0 || !text) return text;
  const re = new RegExp(terms.map(escapeRegExp).join("|"), "giu");
  const out: ReactNode[] = [];
  let last = 0;
  let key = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    if (m[0].length === 0) {
      re.lastIndex++;
      continue;
    }
    if (m.index > last) out.push(text.slice(last, m.index));
    out.push(
      <mark key={key++} className="search-mark">
        {m[0]}
      </mark>,
    );
    last = m.index + m[0].length;
  }
  if (out.length === 0) return text;
  if (last < text.length) out.push(text.slice(last));
  return out;
}
