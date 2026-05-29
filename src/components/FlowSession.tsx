import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Check } from "lucide-react";
import { ComboKeys } from "@/components/ComboKeys";
import { Card, CardContent } from "@/components/ui/card";
import {
  coachLog,
  practiceCompleteSession,
  practiceEnd,
  practiceStartSession,
  practiceSubmitResult,
} from "@/lib/api";
import type { PracticeCard, SentenceToken } from "@/lib/types";
import { usePracticeGate } from "@/hooks/usePracticeGate";
import { matchNorm } from "@/lib/practiceMatch";
import { cn } from "@/lib/utils";

/** Delay before the chord (combo) hint is revealed for the current word.
 *  Mirrors Recall's HINT_DELAY_MS — revealing it discounts first-try credit. */
const HINT_DELAY_MS = 4000;
/** Min gap (ms) since the previous keystroke for an edit to count as a USER
 *  action. Compound chords/arpeggios emit their output (chars + corrective
 *  backspaces) as a synthetic burst <10ms apart; edits within this window are
 *  device output, not user fumbles, so they don't count. */
const BURST_MS = 80;

/**
 * Flow mode: the look-ahead drill. Renders the due-queue phrases as one
 * continuous line (completed words muted, current word highlighted, upcoming
 * words dimmed) and grades the chorded/typed stream one word at a time. Both
 * modes feed the same SR system via practiceSubmitResult; Flow measures
 * per-word flow latency (look-ahead overlap included — intended).
 *
 * Self-contained session lifecycle: it starts its own session on mount and
 * completes it on finish/quit/unmount, so Recall mode stays untouched.
 */
export function FlowSession({
  queue,
  sentence,
  lineCap,
  onQuit,
  onComplete,
  onRepComplete,
}: {
  queue: PracticeCard[];
  /** When provided, drill these sentence tokens (mixed library + glue) instead
   *  of the `queue` phrases. Glue tokens advance on match but are NOT graded;
   *  library tokens are graded + submitted to SR exactly like a queue phrase. */
  sentence?: SentenceToken[];
  /** Max tokens laid into a single Flow line. Queue mode caps at the size-mapped
   *  count (S=8/M=14/L=24); sentence mode passes a large cap (the generated
   *  length already reflects the size, so it isn't truncated). */
  lineCap: number;
  /** User left the session early (End session / Escape). */
  onQuit: () => void;
  /** Last word completed — the session is done. Hands the just-completed
   *  session id and the whole-session WPM to the parent so it can fetch the
   *  per-word recap and show the overall pace. */
  onComplete: (sessionId: number, wpm: number) => void;
  /** Fired once per committed word so the parent can refresh the live header. */
  onRepComplete?: () => void;
}) {
  // Sentence mode drills the token texts; queue mode drills the phrases. Both
  // are frozen for the session's life so a refresh elsewhere can't shift
  // indices mid-drill. In sentence mode every token is graded; in queue mode no
  // token is glue (all are library phrases).
  const words = useMemo(
    () =>
      sentence
        ? sentence.slice(0, lineCap).map((t) => t.text)
        : queue.slice(0, lineCap).map((c) => c.phrase),
    [queue, sentence, lineCap],
  );
  // Whether each laid-in token is glue (skipped for SR). Queue phrases are never
  // glue. Aligned 1:1 with `words`.
  const glueByIndex = useMemo(
    () =>
      sentence
        ? sentence.slice(0, lineCap).map((t) => t.is_glue)
        : queue.slice(0, lineCap).map(() => false),
    [queue, sentence, lineCap],
  );
  // Combos to hint per index. Sentence tokens carry their direct chord mapping
  // (empty for glue/inflection/novel); queue cards carry their device combos.
  const combosByIndex = useMemo(
    () =>
      sentence
        ? sentence.slice(0, lineCap).map((t) => (t.combo ? [t.combo] : []))
        : queue.slice(0, lineCap).map((c) => c.combos),
    [queue, sentence, lineCap],
  );
  // Base lemma per index for inflected sentence tokens ("changing" → "change"),
  // surfaced as a hint so the user knows which base chord to use. Empty for
  // direct chords, glue, novel words, and all queue phrases.
  const baseWordByIndex = useMemo(
    () =>
      sentence
        ? sentence.slice(0, lineCap).map((t) => t.base_word)
        : queue.slice(0, lineCap).map(() => ""),
    [queue, sentence, lineCap],
  );
  // Chord mapping for the base lemma (inflections) — shown alongside the base
  // word so the hint gives both the lemma AND the chord to fire for it.
  const baseComboByIndex = useMemo(
    () =>
      sentence
        ? sentence.slice(0, lineCap).map((t) => t.base_combo)
        : queue.slice(0, lineCap).map(() => ""),
    [queue, sentence, lineCap],
  );

  const [wordIndex, setWordIndex] = useState(0);
  const [hintShown, setHintShown] = useState(false);

  const inputRef = useRef<HTMLInputElement>(null);
  // The currently-active word span, scrolled into view as the drill advances so
  // a long (scrollable) line keeps the current word visible.
  const currentWordRef = useRef<HTMLSpanElement>(null);
  const sessionIdRef = useRef<number | null>(null);
  // Guard so practice_end + complete run exactly once on session leave.
  const inPracticeRef = useRef(false);
  // performance.now() when the current word became active (look-ahead start).
  const wordStartRef = useRef(0);
  // performance.now() when the FIRST word was armed (whole-session start), used
  // to compute the overall session WPM in finishSession.
  const sessionStartRef = useRef(0);
  // Accumulated char length of every COMMITTED token (incl. glue) — the
  // numerator (chars/5) for the session WPM.
  const totalCharsRef = useRef(0);
  // Backspace within / non-prefix divergence of the in-progress current word.
  const hadCorrectionRef = useRef(false);
  // Hint revealed for the current word (discounts its first-try credit).
  const hintShownRef = useRef(false);
  const hintTimerRef = useRef<number | null>(null);
  // High-water mark of committed (submitted) words. The box is NEVER cleared
  // mid-line — clearing breaks the device's arpeggiate model (it backspaces over
  // text IT typed to recapitalize; if we'd cleared, those backspaces hit the
  // wrong content and the retype dup'd onto the next word). Instead the box holds
  // the whole typed line and the device owns every edit (incl. its deletions);
  // we re-walk the full transcript each event and only advance this counter
  // forward, so a transient delete→recapitalize never re-submits or dups.
  const committedRef = useRef(0);
  // How many whitespace segments of the box the committed words occupy. The
  // matcher walks forward from HERE (not from word 0): committed words are never
  // re-validated, so a corrupted/stale earlier segment in the never-cleared box
  // can't block progress on the current word. (Intra-word arpeggiate edits —
  // capitalize, apostrophe — don't change a word's segment count, so this offset
  // stays valid across them.)
  const committedSegsRef = useRef(0);
  // Per-word stat counters (reset on each newly committed word).
  const backspacesRef = useRef(0);
  const correctionsRef = useRef(0);
  // Previous box length, to detect deletions (shrinks) across change events.
  const prevLenRef = useRef(0);
  // Timestamp of the previous input event, to tell device bursts from user edits.
  const lastInputTsRef = useRef(0);

  // Practice gate, driven by input + window focus (releases when the app loses
  // focus so coaching resumes while the user is in another app).
  const { active: focused, onInputFocus, onInputBlur } = usePracticeGate(
    words[wordIndex],
  );

  const focusInput = useCallback(() => {
    requestAnimationFrame(() => inputRef.current?.focus());
  }, []);

  // Keep the current word in view as the drill advances (long lines scroll).
  useEffect(() => {
    currentWordRef.current?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [wordIndex]);

  // (Re)arm the per-word hint timer and reset that word's tracking.
  const armWord = useCallback(() => {
    if (hintTimerRef.current != null) window.clearTimeout(hintTimerRef.current);
    setHintShown(false);
    hintShownRef.current = false;
    hadCorrectionRef.current = false;
    backspacesRef.current = 0;
    correctionsRef.current = 0;
    wordStartRef.current = performance.now();
    lastInputTsRef.current = performance.now();
    hintTimerRef.current = window.setTimeout(() => {
      hintShownRef.current = true;
      setHintShown(true);
    }, HINT_DELAY_MS);
  }, []);

  // Start the session once on mount; complete it on unmount (safety net for the
  // focus-tied gate — practiceEnd is idempotent on the backend).
  useEffect(() => {
    let cancelled = false;
    void practiceStartSession()
      .then((sid) => {
        if (cancelled) {
          void practiceCompleteSession(sid).catch(() => undefined);
          return;
        }
        sessionIdRef.current = sid;
        inPracticeRef.current = true;
        void coachLog(`[PRACTICE-FE] flow session start queue=${words.length}`);
        // Whole-session clock starts when the first word is armed.
        sessionStartRef.current = performance.now();
        totalCharsRef.current = 0;
        armWord();
        focusInput();
      })
      .catch(() => {
        if (!cancelled) onQuit();
      });
    return () => {
      cancelled = true;
      if (hintTimerRef.current != null) window.clearTimeout(hintTimerRef.current);
      if (inPracticeRef.current) {
        void coachLog("[PRACTICE-FE] end reason=unmount");
        inPracticeRef.current = false;
        const sid = sessionIdRef.current;
        if (sid != null) void practiceCompleteSession(sid).catch(() => undefined);
      }
      // The practice gate is released by usePracticeGate's own unmount cleanup.
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const finishSession = useCallback(() => {
    if (hintTimerRef.current != null) {
      window.clearTimeout(hintTimerRef.current);
      hintTimerRef.current = null;
    }
    void coachLog(`[PRACTICE-FE] end reason=flow-complete count=${words.length}`);
    inPracticeRef.current = false;
    const sid = sessionIdRef.current;
    if (sid != null) void practiceCompleteSession(sid).catch(() => undefined);
    void practiceEnd().catch(() => undefined);
    // Whole-session WPM: standard chars/5 over elapsed wall-clock minutes across
    // the entire session (look-ahead overlap included — that's the point).
    const elapsedMin = (performance.now() - sessionStartRef.current) / 60000;
    const wpm =
      elapsedMin > 0 ? Math.round(totalCharsRef.current / 5 / elapsedMin) : 0;
    if (sid != null) onComplete(sid, wpm);
  }, [words.length, onComplete]);

  const handleQuit = useCallback(() => {
    if (hintTimerRef.current != null) {
      window.clearTimeout(hintTimerRef.current);
      hintTimerRef.current = null;
    }
    void coachLog("[PRACTICE-FE] end reason=quit");
    inPracticeRef.current = false;
    const sid = sessionIdRef.current;
    if (sid != null) void practiceCompleteSession(sid).catch(() => undefined);
    void practiceEnd().catch(() => undefined);
    onQuit();
  }, [onQuit]);

  // Submit a single completed word to the SR system (or skip it, for glue
  // tokens) and tally its chars for the session WPM. Index/box/hint advancement
  // is handled by handleChange after it walks the buffer.
  const submitWord = useCallback(
    (idx: number) => {
      const fireMs = Math.max(
        0,
        Math.round(performance.now() - wordStartRef.current),
      );
      // First-try credit is gated only on whether the hint was revealed — NOT on
      // backspaces/corrections. Arpeggios roll through transient non-matching
      // states (and device backspaces) before settling correct; penalizing those
      // would wrongly fail a clean arpeggio. Raw counts are still recorded.
      const firstTry = !hintShownRef.current;
      const sid = sessionIdRef.current;
      // Glue/unknown sentence tokens advance the line but are never graded.
      const isGlue = glueByIndex[idx] ?? false;
      if (sid != null && !isGlue) {
        void practiceSubmitResult(
          sid,
          words[idx].toLowerCase(),
          true,
          firstTry,
          fireMs,
          backspacesRef.current,
          correctionsRef.current,
          hintShownRef.current,
        ).catch(() => undefined);
      }
      onRepComplete?.();
      // Accumulate this token's chars (incl. glue) for the whole-session WPM.
      totalCharsRef.current += words[idx].length;
    },
    [words, glueByIndex, onRepComplete],
  );

  // Grade by re-reading the WHOLE box every input event and walking the expected
  // words from the START of the line. The box is never cleared mid-line, so the
  // transcript is always the device's true output (including any backspaces it
  // makes to recapitalize a word). Re-deriving progress from scratch each event
  // — rather than clearing per word — is what makes this immune to the chord/
  // arpeggiate races (ghost words, the capitalized-word dup).
  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const transcript = e.target.value;
      const prevLen = prevLenRef.current;
      prevLenRef.current = transcript.length;

      const now = performance.now();
      const userEdit = now - lastInputTsRef.current >= BURST_MS;
      lastInputTsRef.current = now;
      if (transcript.length < prevLen && userEdit) {
        backspacesRef.current += 1;
        hadCorrectionRef.current = true;
      }

      const endsWithSpace = /\s$/.test(transcript);
      const segs = transcript.split(/\s+/).filter((s) => s.length > 0);

      // Walk forward from the committed high-water mark, consuming segments. A
      // word commits on an exact (normalized) match; a multi-word phrase target
      // ("of the") consumes several segments. Starting at the committed offset
      // (not 0) means an earlier non-matching segment can't block the current
      // word — committed words are skipped, never re-checked.
      let wi = committedRef.current;
      let si = committedSegsRef.current;
      let mismatch = false;
      while (wi < words.length) {
        const target = words[wi].trim().toLowerCase();
        // Space-insensitive normalized target: matchNorm strips punctuation
        // (incl. apostrophes), and we also drop spaces so a base chord + typed
        // suffix like "person" + "'s" (which lands as TWO segments) still matches
        // the single token "person's". Multi-word phrase targets work too — their
        // segments just concatenate to the same space-stripped form.
        const targetNS = matchNorm(target).replace(/\s+/g, "");
        const isLastWord = wi === words.length - 1;

        // Pure-punctuation token (e.g. "()"): satisfied by any one segment.
        if (targetNS.length === 0) {
          if (si >= segs.length) break;
          const terminated = si + 1 < segs.length || endsWithSpace;
          if (!terminated && !isLastWord) break;
          wi += 1;
          si += 1;
          continue;
        }

        // Greedily consume segments until their space-stripped normalized
        // concatenation equals the target. Stop at the MINIMAL match so we never
        // eat into the next word; bail as soon as the accumulation diverges.
        let consumed = 0;
        let acc = "";
        let matched = false;
        while (si + consumed < segs.length) {
          consumed += 1;
          acc = matchNorm(segs.slice(si, si + consumed).join(" ")).replace(
            /\s+/g,
            "",
          );
          if (acc === targetNS) {
            matched = true;
            break;
          }
          if (!targetNS.startsWith(acc)) break; // diverged — wrong word
        }
        if (!matched) {
          // Diverged → a genuine mismatch the user must fix; still a prefix →
          // just not finished typing yet (wait for more input).
          if (acc.length > 0 && !targetNS.startsWith(acc)) mismatch = true;
          break;
        }

        // Require a TERMINATOR — a following segment or a trailing space — before
        // committing, except for the final word (nothing follows it). This fixes
        // the capitalized-first-word dup: an arpeggio types the lowercase word,
        // DELETES it, then retypes the Capitalized form. Both match, so committing
        // on the first UNTERMINATED match advanced past the word, and the
        // recapitalized retype then spilled onto the next word. Waiting for the
        // space means we commit only once the arpeggio has settled.
        const consumedEnd = si + consumed;
        const terminated = consumedEnd < segs.length || endsWithSpace;
        if (!terminated && !isLastWord) break;
        wi += 1;
        si = consumedEnd;
      }
      const newCommitted = wi;

      // Submit + advance any words completed since the last event. `si` now sits
      // just past the last committed word's segments — record it so the next
      // event resumes from there.
      if (newCommitted > committedRef.current) {
        for (let w = committedRef.current; w < newCommitted; w++) submitWord(w);
        committedRef.current = newCommitted;
        committedSegsRef.current = si;
        armWord(); // re-arm the hint + reset per-word counters for the new word
        setWordIndex(newCommitted);
      }

      if (newCommitted >= words.length) {
        finishSession();
        if (inputRef.current) inputRef.current.value = "";
        prevLenRef.current = 0;
        return;
      }

      // Correction stat: an in-progress fragment for the current word that
      // diverges from it (genuine user edits only; best-effort telemetry).
      if (!mismatch && si < segs.length) {
        const partial = matchNorm(segs.slice(si).join(" "));
        const curTarget = matchNorm(words[newCommitted].trim().toLowerCase());
        if (
          userEdit &&
          partial.length > 0 &&
          curTarget.length > 0 &&
          !curTarget.startsWith(partial)
        ) {
          correctionsRef.current += 1;
          hadCorrectionRef.current = true;
        }
      }
    },
    [words, submitWord, armWord, finishSession],
  );

  return (
    <motion.div
      key="flow"
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -8 }}
      transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }}
      className="relative mt-6 flex flex-1 flex-col outline-none"
    >
      {!focused && (
        <button
          type="button"
          onClick={() => inputRef.current?.focus()}
          className="absolute inset-0 z-10 flex flex-col items-center justify-center gap-1 rounded-xl bg-background/70 text-center backdrop-blur-sm"
        >
          <span className="text-sm font-medium text-foreground">Click to focus</span>
          <span className="text-xs text-muted-foreground">
            Keep the box focused to read ahead and chord the line.
          </span>
        </button>
      )}

      <div className="mb-3 flex items-center justify-between text-xs text-muted-foreground">
        <span className="tnum">
          Word {Math.min(wordIndex + 1, words.length)} of {words.length}
        </span>
        <span className="tnum">{Math.max(0, words.length - wordIndex - 1)} remaining</span>
      </div>

      <Card className="flex flex-1 flex-col items-center justify-center gap-8 py-12">
        <CardContent className="flex w-full flex-col items-center gap-8">
          <p className="text-xs tracking-wider text-muted-foreground/70 uppercase">
            Read ahead — chord the line
          </p>

          {/* The continuous look-ahead line. Long lines (e.g. a verbose model)
              scroll within a capped height instead of overflowing the viewport;
              the current word is kept in view as you advance (see effect). */}
          <div className="flex max-h-[42vh] max-w-3xl flex-wrap items-baseline justify-center gap-x-3 gap-y-2 overflow-y-auto font-mono text-3xl leading-relaxed tracking-[-0.01em]">
            {words.map((word, i) => {
              const done = i < wordIndex;
              const current = i === wordIndex;
              // Novel (non-chord) words in sentence mode: typed but not graded.
              // A faint dotted underline cues "no chord for this word yet" — an
              // expansion hint — without disrupting the done/current/upcoming
              // styling. Suppressed on the current word (its gold underline wins).
              const novel = (glueByIndex[i] ?? false) && !current;
              return (
                <span
                  key={`${word}-${i}`}
                  ref={current ? currentWordRef : undefined}
                  className={cn(
                    "inline-flex items-center gap-1.5 transition-colors duration-200",
                    done && "text-muted-foreground/40",
                    current && "font-semibold text-foreground",
                    !done && !current && "text-muted-foreground/55",
                  )}
                >
                  {done && <Check className="size-4 text-success/60" strokeWidth={2.4} />}
                  <span
                    className={cn(
                      current && "underline decoration-gold/70 decoration-2 underline-offset-8",
                      novel &&
                        "underline decoration-dotted decoration-muted-foreground/30 underline-offset-[6px]",
                    )}
                    title={novel ? "No chord for this word yet" : undefined}
                  >
                    {/* Cosmetic: capitalize the first word of the line so it
                        reads like a sentence. Matching stays case-insensitive
                        (it compares `words`, untouched), so this is display-only. */}
                    {i === 0 ? word.charAt(0).toUpperCase() + word.slice(1) : word}
                  </span>
                </span>
              );
            })}
          </div>

          <input
            ref={inputRef}
            defaultValue=""
            onChange={handleChange}
            onFocus={onInputFocus}
            onBlur={onInputBlur}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                handleQuit();
              }
            }}
            autoComplete="off"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck={false}
            placeholder="Type or chord the line…"
            aria-label="Flow practice input"
            className="w-full max-w-md rounded-lg border border-border bg-secondary/40 px-4 py-2.5 text-center font-mono text-lg text-foreground outline-none transition-colors placeholder:text-muted-foreground/50 focus:border-foreground/30 focus:bg-secondary/60"
          />

          <div className="flex h-12 items-center">
            {/* Hints reveal only AFTER the per-word timeout (HINT_DELAY_MS), for
                every word: a direct chord shows its mapping; an inflection shows
                its base lemma AND that lemma's chord to fire. */}
            <AnimatePresence mode="wait">
              {!hintShown ? null : combosByIndex[wordIndex]?.length > 0 ? (
                <motion.div
                  key={`hint-${wordIndex}`}
                  initial={{ opacity: 0, y: 6 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -6 }}
                  transition={{ duration: 0.25 }}
                  className="flex flex-col items-center gap-1.5"
                >
                  <span className="text-[10px] tracking-wider text-muted-foreground/60 uppercase">
                    Hint
                  </span>
                  {combosByIndex[wordIndex].map((combo, i) => (
                    <ComboKeys key={`${combo}-${i}`} combo={combo} />
                  ))}
                </motion.div>
              ) : baseWordByIndex[wordIndex] ? (
                // Inflected token: no direct chord. Show the base lemma the user
                // DOES have a chord for ("changing" → "change") AND that lemma's
                // mapping — chord the base, then type the inflection.
                <motion.div
                  key={`base-${wordIndex}`}
                  initial={{ opacity: 0, y: 6 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -6 }}
                  transition={{ duration: 0.25 }}
                  className="flex flex-col items-center gap-1.5"
                >
                  <div className="flex items-center gap-2">
                    <span className="text-[10px] tracking-wider text-muted-foreground/60 uppercase">
                      Base chord
                    </span>
                    <span className="font-mono text-sm text-foreground">
                      {baseWordByIndex[wordIndex]}
                    </span>
                  </div>
                  {baseComboByIndex[wordIndex] ? (
                    <ComboKeys combo={baseComboByIndex[wordIndex]} />
                  ) : null}
                </motion.div>
              ) : null}
            </AnimatePresence>
          </div>
        </CardContent>
      </Card>
    </motion.div>
  );
}
