// Shared normalization for practice-drill matching.
//
// Two problems this solves:
//  1. Chorded paired punctuation emits BOTH marks at once and drops the caret
//     BETWEEN them: `()` → `(|)`, then the user types the inner content. So a
//     target like `(example)` is built as `()` → `(e)` → `(ex)` → `(example)` —
//     none of the intermediate states are left-to-right prefixes of the target.
//  2. In Sentence mode the model attaches punctuation to words ("task,",
//     "constraints!", a leading quote on "While"). A chord produces the BARE
//     word, so the chorded "task" must still complete the token "task,".
//
// Stripping wrapping + sentence punctuation from BOTH sides makes the prefix +
// equality checks compare the alphanumeric core, fixing both. Normal chord
// targets (Recall / queue-Flow) are bare words, so stripping is a no-op for them.
// Contractions lose their apostrophe ("don't" → "dont") on both sides, so they
// still match.

/** Punctuation to ignore when matching: paired/wrapping marks a single chord can
 *  emit around content — `()[]{}` and ASCII + smart quotes (“ ” ‘ ’) — plus the
 *  sentence punctuation a model attaches to words (, . ! ? ; : … and dashes),
 *  which a chord never produces. */
const STRIP_CHARS = /[()[\]{}"'‘’“”,.!?;:…–—-]/g;

/**
 * Lowercase, strip wrapping + sentence punctuation entirely, then trim.
 * Used for both the completion-equality and on-track/prefix comparisons.
 */
export function matchNorm(s: string): string {
  return s.toLowerCase().replace(STRIP_CHARS, "").trim();
}
