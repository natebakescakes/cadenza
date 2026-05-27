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
}: {
  queue: PracticeCard[];
  /** User left the session early (End session / Escape). */
  onQuit: () => void;
  /** Last word completed — the session is done. */
  onComplete: () => void;
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
  // Live current word index for callbacks that mustn't close over stale state.
  const wordIndexRef = useRef(0);
  wordIndexRef.current = wordIndex;

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
    onComplete();
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

  // Grade the chorded/typed stream one word at a time. A word is COMPLETE when
  // the user has produced `target[wordIndex]` followed by a space (or it's the
  // last word and matches exactly). Chording a word emits word + trailing space,
  // so a completed non-final word arrives as "<word> " in the stream.
  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const next = e.target.value;
      const idx = wordIndexRef.current;
      const target = words[idx];
      if (target == null) {
        setValue(next);
        return;
      }
      const prevLen = value.length;
      const targetLc = target.trim().toLowerCase();

      // The in-progress fragment is whatever follows the last space in the box.
      const lastSpace = next.lastIndexOf(" ");
      const fragment = (lastSpace >= 0 ? next.slice(lastSpace + 1) : next)
        .trim()
        .toLowerCase();

      // Correction tracking on the in-progress word: a shrink (backspace) or a
      // fragment that is no longer a case-insensitive prefix of the target word.
      if (next.length < prevLen) {
        hadCorrectionRef.current = true;
      } else if (fragment.length > 0 && !targetLc.startsWith(fragment)) {
        hadCorrectionRef.current = true;
      }
      setValue(next);

      const isLast = idx === words.length - 1;
      // Non-final word completes when the chord's trailing space lands after a
      // matching fragment; final word completes on an exact (trimmed) match.
      const wholeLc = next.trim().toLowerCase();
      const completed = isLast
        ? wholeLc === targetLc
        : next.includes(" ") && fragment === targetLc;
      // Guard: a trailing space alone (empty fragment) is not a completion.
      if (!completed || (!isLast && fragment.length === 0)) return;

      const fireMs = Math.max(
        0,
        Math.round(performance.now() - wordStartRef.current),
      );
      const firstTry = !hadCorrectionRef.current && !hintShownRef.current;
      if (hintTimerRef.current != null) {
        window.clearTimeout(hintTimerRef.current);
        hintTimerRef.current = null;
      }
      const sid = sessionIdRef.current;
      if (sid != null) {
        void practiceSubmitResult(sid, target, true, firstTry, fireMs).catch(
          () => undefined,
        );
      }

      if (isLast) {
        setValue("");
        finishSession();
        return;
      }

      // Advance: clear the typed fragment, re-arm tracking + hint timer. The
      // gate's phrase tracks words[wordIndex] live via usePracticeGate.
      const nextIdx = idx + 1;
      setWordIndex(nextIdx);
      setValue("");
      armWord();
    },
    [words, value, armWord, finishSession],
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
