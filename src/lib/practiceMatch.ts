// Shared normalization for practice-drill matching.
//
// Chorded paired punctuation emits BOTH marks at once and drops the caret
// BETWEEN them: `()` → `(|)`, then the user types the inner content. So a target
// like `(example)` is built as `()` → `(e)` → `(ex)` → `(example)` — none of the
// intermediate states are left-to-right prefixes of the target. Stripping the
// wrapping/paired punctuation from BOTH sides makes the prefix + equality checks
// tolerate this so a caret-inside intermediate isn't flagged as a correction.
//
// Normal words are unaffected: matchNorm("hello") === "hello". Contractions lose
// their apostrophe ("don't" → "dont") on both sides, so they still match.

/** Paired/wrapping punctuation a single chord can emit around its content. */
const WRAP_CHARS = /[()[\]{}"']/g;

/**
 * Lowercase, strip wrapping/paired punctuation entirely, then trim.
 * Used for both the completion-equality and on-track/prefix comparisons.
 */
export function matchNorm(s: string): string {
  return s.toLowerCase().replace(WRAP_CHARS, "").trim();
}
