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
import type { PracticeCard } from "@/lib/types";
import { usePracticeGate } from "@/hooks/usePracticeGate";
import { cn } from "@/lib/utils";

/** Delay before the chord (combo) hint is revealed for the current word.
 *  Mirrors Recall's HINT_DELAY_MS — revealing it discounts first-try credit. */
const HINT_DELAY_MS = 4000;
/** Cap how many queued phrases get laid into a single Flow line. */
const FLOW_LINE_CAP = 14;

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
  onQuit,
  onComplete,
  onRepComplete,
}: {
  queue: PracticeCard[];
  /** User left the session early (End session / Escape). */
  onQuit: () => void;
  /** Last word completed — the session is done. Hands the just-completed
   *  session id to the parent so it can fetch the per-word recap. */
  onComplete: (sessionId: number) => void;
  /** Fired once per committed word so the parent can refresh the live header. */
  onRepComplete?: () => void;
}) {
  // The phrases laid into the line (capped). Frozen for the session's life so
  // a queue refresh elsewhere can't shift indices mid-drill.
  const words = useMemo(
    () => queue.slice(0, FLOW_LINE_CAP).map((c) => c.phrase),
    [queue],
  );
  const combosByIndex = useMemo(
    () => queue.slice(0, FLOW_LINE_CAP).map((c) => c.combos),
    [queue],
  );

  const [wordIndex, setWordIndex] = useState(0);
  const [value, setValue] = useState("");
  const [hintShown, setHintShown] = useState(false);

  const inputRef = useRef<HTMLInputElement>(null);
  const sessionIdRef = useRef<number | null>(null);
  // Guard so practice_end + complete run exactly once on session leave.
  const inPracticeRef = useRef(false);
  // performance.now() when the current word became active (look-ahead start).
  const wordStartRef = useRef(0);
  // Backspace within / non-prefix divergence of the in-progress current word.
  const hadCorrectionRef = useRef(false);
  // Hint revealed for the current word (discounts its first-try credit).
  const hintShownRef = useRef(false);
  const hintTimerRef = useRef<number | null>(null);
  // Committed-word count (words finalized by a following space). Drives the
  // look-ahead highlight; the input box keeps the FULL typed line.
  const committedRef = useRef(0);
  // Per-word stat counters (reset on each newly committed word).
  const backspacesRef = useRef(0);
  const correctionsRef = useRef(0);
  // Previous box length, to detect deletions (shrinks) across change events.
  const prevLenRef = useRef(0);

  // Practice gate, driven by input + window focus (releases when the app loses
  // focus so coaching resumes while the user is in another app).
  const { active: focused, onInputFocus, onInputBlur } = usePracticeGate(
    words[wordIndex],
  );

  const focusInput = useCallback(() => {
    requestAnimationFrame(() => inputRef.current?.focus());
  }, []);

  // (Re)arm the per-word hint timer and reset that word's tracking.
  const armWord = useCallback(() => {
    if (hintTimerRef.current != null) window.clearTimeout(hintTimerRef.current);
    setHintShown(false);
    hintShownRef.current = false;
    hadCorrectionRef.current = false;
    backspacesRef.current = 0;
    correctionsRef.current = 0;
    wordStartRef.current = performance.now();
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
    if (sid != null) onComplete(sid);
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

  // Grade the current phrase from the box, which holds ONLY the current phrase's
  // input (cleared on advance). The whole target is compared as a string (so
  // multi-word phrases work). A phrase COMPLETES on an exact match with the
  // trailing space trimmed — so an arpeggio (emits the phrase with NO trailing
  // space) completes just like a chord (phrase + trailing space). Advancing
  // REQUIRES a correct match: a diverged entry followed by a space is a wrong
  // submission — it's flagged and the box is cleared to retype, never advanced.
  const commitWord = useCallback(
    (idx: number) => {
      const fireMs = Math.max(
        0,
        Math.round(performance.now() - wordStartRef.current),
      );
      const firstTry = !hadCorrectionRef.current && !hintShownRef.current;
      const sid = sessionIdRef.current;
      if (sid != null) {
        void practiceSubmitResult(
          sid,
          words[idx],
          true,
          firstTry,
          fireMs,
          backspacesRef.current,
          correctionsRef.current,
          hintShownRef.current,
        ).catch(() => undefined);
      }
      onRepComplete?.();
      committedRef.current += 1;
      prevLenRef.current = 0;
      setValue("");
      setWordIndex(committedRef.current);
      if (committedRef.current >= words.length) {
        finishSession();
      } else {
        armWord();
      }
    },
    [words, armWord, finishSession, onRepComplete],
  );

  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const newValue = e.target.value;
      const prevLen = prevLenRef.current;
      prevLenRef.current = newValue.length;

      const idx = committedRef.current;
      if (idx >= words.length) return;
      const target = words[idx].trim().toLowerCase();
      // Drop trailing whitespace only (chords append a space; keep internal
      // spaces so multi-word phrases match).
      const typed = newValue.toLowerCase().replace(/\s+$/, "");
      const isPrefix = target.startsWith(typed);

      // Edit stats for the in-progress phrase.
      if (newValue.length < prevLen) {
        backspacesRef.current += 1;
        hadCorrectionRef.current = true;
      } else if (typed.length > 0 && !isPrefix) {
        correctionsRef.current += 1;
        hadCorrectionRef.current = true;
      }

      // Correct phrase produced (chord or arpeggio or typed) → commit + advance.
      if (typed.length > 0 && typed === target) {
        commitWord(idx);
        return;
      }
      // Diverged AND a trailing space was typed → a wrong submission: flag it
      // and clear so the user retypes this phrase. Do NOT advance.
      if (typed.length > 0 && !isPrefix && /\s$/.test(newValue)) {
        correctionsRef.current += 1;
        hadCorrectionRef.current = true;
        prevLenRef.current = 0;
        setValue("");
        return;
      }
      // Stray leading space(s) / empty → consume.
      if (typed.length === 0) {
        prevLenRef.current = 0;
        setValue("");
        return;
      }
      // Still typing (a valid prefix, or diverged-without-space so they can fix).
      setValue(newValue);
    },
    [words, commitWord],
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
          Word {wordIndex + 1} of {words.length}
        </span>
        <span className="tnum">{words.length - wordIndex - 1} remaining</span>
      </div>

      <Card className="flex flex-1 flex-col items-center justify-center gap-8 py-12">
        <CardContent className="flex w-full flex-col items-center gap-8">
          <p className="text-xs tracking-wider text-muted-foreground/70 uppercase">
            Read ahead — chord the line
          </p>

          {/* The continuous look-ahead line. */}
          <div className="flex max-w-3xl flex-wrap items-baseline justify-center gap-x-3 gap-y-2 font-mono text-3xl leading-relaxed tracking-[-0.01em]">
            {words.map((word, i) => {
              const done = i < wordIndex;
              const current = i === wordIndex;
              return (
                <span
                  key={`${word}-${i}`}
                  className={cn(
                    "inline-flex items-center gap-1.5 transition-colors duration-200",
                    done && "text-muted-foreground/40",
                    current && "font-semibold text-foreground",
                    !done && !current && "text-muted-foreground/55",
                  )}
                >
                  {done && <Check className="size-4 text-success/60" strokeWidth={2.4} />}
                  <span className={cn(current && "underline decoration-gold/70 decoration-2 underline-offset-8")}>
                    {word}
                  </span>
                </span>
              );
            })}
          </div>

          <input
            ref={inputRef}
            value={value}
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

          <div className="flex h-10 items-center">
            <AnimatePresence mode="wait">
              {hintShown && combosByIndex[wordIndex]?.length > 0 && (
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
              )}
            </AnimatePresence>
          </div>
        </CardContent>
      </Card>
    </motion.div>
  );
}
