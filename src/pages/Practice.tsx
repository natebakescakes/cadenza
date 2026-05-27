import { useCallback, useEffect, useRef, useState } from "react";
import { AnimatePresence, motion, type Variants } from "framer-motion";
import {
  Check,
  Dumbbell,
  Flame,
  Gauge,
  Layers,
  Sparkles,
  Target,
  Zap,
} from "lucide-react";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { ComboKeys } from "@/components/ComboKeys";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  coachLog,
  practiceBegin,
  practiceCardStats,
  practiceCompleteSession,
  practiceDueQueue,
  practiceEnd,
  practiceOverview,
  practiceStartSession,
  practiceSubmitResult,
} from "@/lib/api";
import { formatMs, formatNumber, formatPercent } from "@/lib/format";
import type {
  PracticeCard,
  PracticeCardStats,
  PracticeOverview,
} from "@/lib/types";
import { cn } from "@/lib/utils";

const QUEUE_LIMIT = 30;
/** Delay before the chord (combo) hint is revealed for a card. Revealing it
 *  before the user completes the card discounts the rep below first-try credit. */
const HINT_DELAY_MS = 4000;

const stagger: Variants = {
  hidden: { opacity: 0, y: 12 },
  show: (i: number) => ({
    opacity: 1,
    y: 0,
    transition: { delay: i * 0.04, duration: 0.4, ease: [0.16, 1, 0.3, 1] },
  }),
};

/** Quick verdict shown after a chord fires during the drill. */
type Feedback = { correct: boolean; fireMs: number; firstTry: boolean } | null;

// --- Overview header ------------------------------------------------------

function OverviewStat({
  icon: Icon,
  label,
  value,
  accent,
}: {
  icon: typeof Flame;
  label: string;
  value: string;
  accent?: boolean;
}) {
  return (
    <div className="flex items-center gap-2.5 rounded-lg border border-border bg-secondary/40 px-3 py-2">
      <Icon
        className={cn("size-4", accent ? "text-gold" : "text-muted-foreground/70")}
        strokeWidth={1.85}
      />
      <div className="leading-none">
        <p className="tnum text-sm font-semibold text-foreground">{value}</p>
        <p className="mt-0.5 text-[10px] tracking-wider text-muted-foreground/70 uppercase">
          {label}
        </p>
      </div>
    </div>
  );
}

function OverviewBar({ overview }: { overview: PracticeOverview | null }) {
  return (
    <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-4">
      <OverviewStat
        icon={Flame}
        label="Streak"
        value={overview ? `${overview.current_streak}d` : "—"}
        accent
      />
      <OverviewStat
        icon={Target}
        label="Due"
        value={overview ? formatNumber(overview.due_count) : "—"}
      />
      <OverviewStat
        icon={Layers}
        label="Cards"
        value={overview ? formatNumber(overview.distinct_cards) : "—"}
      />
      <OverviewStat
        icon={Zap}
        label="Total reps"
        value={overview ? formatNumber(overview.total_reps) : "—"}
      />
    </div>
  );
}

// --- Queue card -----------------------------------------------------------

function QueueCard({ card, index }: { card: PracticeCard; index: number }) {
  return (
    <motion.div
      custom={index}
      initial="hidden"
      animate="show"
      variants={stagger}
    >
      <Card className="h-full gap-3 py-4 transition-colors hover:ring-foreground/20">
        <CardContent className="space-y-3">
          <div className="flex items-center justify-between gap-2">
            <span className="truncate font-mono text-sm font-medium text-foreground">
              {card.phrase}
            </span>
            {card.is_new ? (
              <Badge variant="outline" className="gap-1 text-gold">
                <Sparkles className="size-3" /> New
              </Badge>
            ) : (
              <Badge variant="outline" className="tnum text-muted-foreground">
                {card.reps}× reps
              </Badge>
            )}
          </div>
          {card.combos.length > 0 ? (
            <div className="flex flex-col gap-1.5 border-t border-border pt-2.5">
              {card.combos.map((combo, i) => (
                <ComboKeys key={`${combo}-${i}`} combo={combo} />
              ))}
            </div>
          ) : (
            <p className="border-t border-border pt-2.5 text-[11px] text-muted-foreground/60">
              No device chord mapped yet.
            </p>
          )}
        </CardContent>
      </Card>
    </motion.div>
  );
}

// --- Per-card stats panel (during drill) ----------------------------------

function StatsPanel({ stats }: { stats: PracticeCardStats | null }) {
  if (!stats) {
    return (
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        {[0, 1, 2, 3].map((i) => (
          <div
            key={i}
            className="h-12 animate-pulse rounded-lg border border-border bg-secondary/40"
          />
        ))}
      </div>
    );
  }
  const cells: { label: string; value: string }[] = [
    { label: "Reps", value: formatNumber(stats.reps) },
    { label: "First-try", value: formatPercent(stats.first_try_accuracy) },
    {
      label: "Recent speed",
      value: stats.recent_avg_fire_ms > 0 ? formatMs(stats.recent_avg_fire_ms) : "—",
    },
    {
      label: "Interval",
      value: stats.interval_days > 0 ? `${stats.interval_days.toFixed(1)}d` : "new",
    },
  ];
  return (
    <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
      {cells.map((c) => (
        <div
          key={c.label}
          className="rounded-lg border border-border bg-secondary/40 px-3 py-2"
        >
          <p className="text-[10px] tracking-wider text-muted-foreground/70 uppercase">
            {c.label}
          </p>
          <p className="tnum mt-0.5 text-sm font-semibold text-foreground">
            {c.value}
          </p>
        </div>
      ))}
    </div>
  );
}

// --- Page -----------------------------------------------------------------

type Phase = "idle" | "drilling" | "done";

export default function Practice() {
  const [queue, setQueue] = useState<PracticeCard[]>([]);
  const [overview, setOverview] = useState<PracticeOverview | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  const [index, setIndex] = useState(0);
  const [feedback, setFeedback] = useState<Feedback>(null);
  const [cardStats, setCardStats] = useState<PracticeCardStats | null>(null);
  const [loading, setLoading] = useState(true);
  // Whether the drill input holds keyboard focus. When false we dim + steer the
  // user back: the practice gate (which suppresses ambient stats + coaching) is
  // tied to this focus, so blurring turns the gate OFF (coaching resumes) and
  // refocusing turns it back ON.
  const [focused, setFocused] = useState(false);
  // Live text the user types/chords into the box (the graded surface).
  const [value, setValue] = useState("");
  // Whether the combo hint has been revealed for the active card (discounts grade).
  const [hintShown, setHintShown] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const focusInput = useCallback(() => {
    // Defer so the element exists after the phase/card render.
    requestAnimationFrame(() => inputRef.current?.focus());
  }, []);

  const sessionIdRef = useRef<number | null>(null);
  // Whether the practice gate is currently ON (begin called, end pending).
  // Guards practiceBegin/End so they fire exactly once per transition.
  const gateOnRef = useRef(false);
  // Wall-clock start (performance.now) of the active card; used for fireMs.
  const cardStartRef = useRef(0);
  // True once the user backspaces or types a non-prefix (wrong) char this card.
  const hadCorrectionRef = useRef(false);
  // True once the combo hint has been revealed for the active card.
  const hintShownRef = useRef(false);
  // Pending hint-reveal timer for the active card.
  const hintTimerRef = useRef<number | null>(null);
  // Guard so practice_end runs exactly once on session leave (unmount/quit/done).
  const inPracticeRef = useRef(false);

  const refreshOverview = useCallback(() => {
    void practiceOverview()
      .then(setOverview)
      .catch(() => undefined);
  }, []);

  const loadQueue = useCallback(() => {
    setLoading(true);
    void practiceDueQueue(QUEUE_LIMIT)
      .then((cards) => setQueue(cards))
      .catch(() => setQueue([]))
      .finally(() => setLoading(false));
  }, []);

  // Load the queue + overview once on mount. (The queue is a spaced-repetition
  // set that changes slowly; it's also reloaded after a session ends. We do NOT
  // refetch on every window focus — that re-ran the heavy proficiency query and
  // flashed the UI every time the window regained focus.)
  useEffect(() => {
    loadQueue();
    refreshOverview();
  }, [loadQueue, refreshOverview]);

  // Always leave practice mode when the page unmounts. practiceEnd() is the
  // safety net for the focus-tied gate (idempotent on the backend).
  useEffect(() => {
    return () => {
      if (hintTimerRef.current != null) window.clearTimeout(hintTimerRef.current);
      if (inPracticeRef.current) {
        void coachLog("[PRACTICE-FE] end reason=unmount");
        inPracticeRef.current = false;
        const sid = sessionIdRef.current;
        if (sid != null) void practiceCompleteSession(sid).catch(() => undefined);
      }
      gateOnRef.current = false;
      void practiceEnd().catch(() => undefined);
    };
  }, []);

  const loadStatsFor = useCallback((phrase: string) => {
    setCardStats(null);
    void practiceCardStats(phrase)
      .then(setCardStats)
      .catch(() => setCardStats(null));
  }, []);

  // Begin a fresh card: reset typed text + tracking, snapshot the start time,
  // and (re)arm the hint-reveal timer.
  const beginCard = useCallback((phrase: string) => {
    if (hintTimerRef.current != null) window.clearTimeout(hintTimerRef.current);
    setValue("");
    setFeedback(null);
    setHintShown(false);
    hintShownRef.current = false;
    hadCorrectionRef.current = false;
    cardStartRef.current = performance.now();
    loadStatsFor(phrase);
    hintTimerRef.current = window.setTimeout(() => {
      hintShownRef.current = true;
      setHintShown(true);
    }, HINT_DELAY_MS);
  }, [loadStatsFor]);

  // Move to the next card or finish the session.
  const advance = useCallback(() => {
    if (hintTimerRef.current != null) {
      window.clearTimeout(hintTimerRef.current);
      hintTimerRef.current = null;
    }
    const next = index + 1;
    if (next >= queue.length) {
      // End of session.
      void coachLog(`[PRACTICE-FE] end reason=queue-complete count=${queue.length}`);
      inPracticeRef.current = false;
      gateOnRef.current = false;
      const sid = sessionIdRef.current;
      if (sid != null) void practiceCompleteSession(sid).catch(() => undefined);
      void practiceEnd().catch(() => undefined);
      setPhase("done");
      setFeedback(null);
      setValue("");
      refreshOverview();
      loadQueue();
      return;
    }
    setIndex(next);
    beginCard(queue[next].phrase);
    focusInput();
  }, [index, queue, beginCard, loadQueue, refreshOverview, focusInput]);

  const advanceRef = useRef(advance);
  advanceRef.current = advance;

  // The practice gate follows the input focus. Begin/end run at most once per
  // transition so leaving the box (e.g. switching apps) turns the gate OFF —
  // letting ambient stats + the coaching overlay resume — and refocusing turns
  // it back ON to suppress them while drilling.
  const currentPhrase = phase === "drilling" ? queue[index]?.phrase : undefined;
  const currentPhraseRef = useRef<string | undefined>(currentPhrase);
  currentPhraseRef.current = currentPhrase;

  const handleFocus = useCallback(() => {
    setFocused(true);
    const phrase = currentPhraseRef.current;
    if (phrase && !gateOnRef.current) {
      gateOnRef.current = true;
      void practiceBegin(phrase).catch(() => undefined);
    }
  }, []);

  const handleBlur = useCallback(() => {
    setFocused(false);
    if (gateOnRef.current) {
      gateOnRef.current = false;
      void practiceEnd().catch(() => undefined);
    }
  }, []);

  // Grade by box content. A wrong char or a backspace flags a correction; an
  // exact (trimmed, case-insensitive) match completes the card.
  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      if (feedback?.correct) return; // already completed; ignore stray input
      const next = e.target.value;
      const target = currentPhraseRef.current;
      if (!target) {
        setValue(next);
        return;
      }
      const prevLen = value.length;
      const trimmed = next.trim().toLowerCase();
      const targetTrimmed = target.trim().toLowerCase();
      // Backspace (shrinking) or a typed prefix that diverges = a correction.
      if (next.length < prevLen) {
        hadCorrectionRef.current = true;
      } else if (trimmed.length > 0 && !targetTrimmed.startsWith(trimmed)) {
        hadCorrectionRef.current = true;
      }
      setValue(next);

      if (trimmed === targetTrimmed) {
        const fireMs = Math.max(0, Math.round(performance.now() - cardStartRef.current));
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
        setFeedback({ correct: true, fireMs, firstTry });
        // Brief beat to show the verdict, then advance.
        window.setTimeout(() => advanceRef.current(), 650);
      }
    },
    [feedback, value],
  );

  const startSession = useCallback(async () => {
    if (!queue.length) return;
    const cards = queue;
    try {
      const sid = await practiceStartSession();
      sessionIdRef.current = sid;
      inPracticeRef.current = true;
      setIndex(0);
      setPhase("drilling");
      beginCard(cards[0].phrase);
      void coachLog(`[PRACTICE-FE] session start queue=${cards.length}`);
      focusInput();
    } catch {
      // If we couldn't enter practice mode, fall back to the queue view.
      inPracticeRef.current = false;
      setPhase("idle");
    }
  }, [queue, beginCard, focusInput]);

  const quitSession = useCallback(() => {
    void coachLog("[PRACTICE-FE] end reason=quit");
    if (hintTimerRef.current != null) {
      window.clearTimeout(hintTimerRef.current);
      hintTimerRef.current = null;
    }
    inPracticeRef.current = false;
    gateOnRef.current = false;
    const sid = sessionIdRef.current;
    if (sid != null) void practiceCompleteSession(sid).catch(() => undefined);
    void practiceEnd().catch(() => undefined);
    sessionIdRef.current = null;
    setPhase("idle");
    setFeedback(null);
    setValue("");
    refreshOverview();
    loadQueue();
  }, [loadQueue, refreshOverview]);

  const currentCard = phase === "drilling" ? queue[index] : undefined;

  return (
    <div className="flex min-h-[calc(100vh-124px)] flex-col">
      <PageHeader
        title="Practice"
        subtitle="Drill your weakest chords — spaced repetition, one at a time."
        actions={
          phase === "drilling" ? (
            <Button variant="ghost" size="sm" onClick={quitSession}>
              End session
            </Button>
          ) : undefined
        }
      />

      <OverviewBar overview={overview} />

      <AnimatePresence mode="wait">
        {phase === "drilling" && currentCard ? (
          <motion.div
            key="drill"
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
                  Keep the box focused to type or chord the phrase.
                </span>
              </button>
            )}
            <div className="mb-3 flex items-center justify-between text-xs text-muted-foreground">
              <span className="tnum">
                Card {index + 1} of {queue.length}
              </span>
              <span className="tnum">
                {queue.length - index - 1} remaining
              </span>
            </div>

            <Card className="flex flex-1 flex-col items-center justify-center gap-6 py-12">
              <CardContent className="flex w-full flex-col items-center gap-6">
                <p className="text-xs tracking-wider text-muted-foreground/70 uppercase">
                  Type or chord this
                </p>
                <motion.span
                  key={currentCard.phrase}
                  initial={{ opacity: 0, scale: 0.96 }}
                  animate={{ opacity: 1, scale: 1 }}
                  transition={{ duration: 0.35, ease: [0.16, 1, 0.3, 1] }}
                  className="font-display text-5xl font-semibold tracking-[-0.02em] text-foreground"
                >
                  {currentCard.phrase}
                </motion.span>

                <input
                  ref={inputRef}
                  value={value}
                  onChange={handleChange}
                  onFocus={handleFocus}
                  onBlur={handleBlur}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") {
                      e.preventDefault();
                      quitSession();
                    }
                  }}
                  autoComplete="off"
                  autoCorrect="off"
                  autoCapitalize="off"
                  spellCheck={false}
                  placeholder="Type or chord here…"
                  aria-label="Practice input"
                  className="w-full max-w-sm rounded-lg border border-border bg-secondary/40 px-4 py-2.5 text-center font-mono text-lg text-foreground outline-none transition-colors placeholder:text-muted-foreground/50 focus:border-foreground/30 focus:bg-secondary/60"
                />

                <div className="flex h-10 items-center">
                  <AnimatePresence mode="wait">
                    {hintShown && currentCard.combos.length > 0 && !feedback?.correct && (
                      <motion.div
                        key="hint"
                        initial={{ opacity: 0, y: 6 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, y: -6 }}
                        transition={{ duration: 0.25 }}
                        className="flex flex-col items-center gap-1.5"
                      >
                        <span className="text-[10px] tracking-wider text-muted-foreground/60 uppercase">
                          Hint
                        </span>
                        {currentCard.combos.map((combo, i) => (
                          <ComboKeys key={`${combo}-${i}`} combo={combo} />
                        ))}
                      </motion.div>
                    )}
                  </AnimatePresence>
                </div>

                <div className="flex h-8 items-center">
                  <AnimatePresence mode="wait">
                    {feedback?.correct && (
                      <motion.div
                        key={feedback.fireMs}
                        initial={{ opacity: 0, y: 6 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, y: -6 }}
                        transition={{ duration: 0.2 }}
                        className="inline-flex items-center gap-1.5 rounded-full border border-success/30 bg-success/10 px-3 py-1 text-xs font-medium text-success"
                      >
                        <Check className="size-3.5" />
                        {feedback.firstTry ? "Nice" : "Got it"} ·{" "}
                        {formatMs(feedback.fireMs)}
                      </motion.div>
                    )}
                  </AnimatePresence>
                </div>
              </CardContent>
            </Card>

            <div className="mt-4">
              <StatsPanel stats={cardStats} />
            </div>
          </motion.div>
        ) : (
          <motion.div
            key="queue"
            initial={{ opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -8 }}
            transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }}
            className="mt-6 flex flex-1 flex-col"
          >
            <div className="mb-4 flex items-center justify-between gap-3">
              <div className="flex items-center gap-2">
                <Gauge className="size-4 text-gold" />
                <h2 className="text-sm font-medium text-foreground">Due now</h2>
                <Badge variant="outline" className="tnum text-muted-foreground">
                  {queue.length}
                </Badge>
              </div>
              <Button
                onClick={() => void startSession()}
                disabled={!queue.length}
              >
                <Dumbbell className="size-4" />
                {phase === "done" ? "Practice again" : "Start session"}
              </Button>
            </div>

            {loading ? (
              <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
                {[0, 1, 2].map((i) => (
                  <div
                    key={i}
                    className="h-28 animate-pulse rounded-xl bg-card ring-1 ring-foreground/10"
                  />
                ))}
              </div>
            ) : queue.length ? (
              <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
                {queue.map((card, i) => (
                  <QueueCard key={card.phrase} card={card} index={i} />
                ))}
              </div>
            ) : (
              <Card className="flex flex-1 items-center justify-center">
                <CardContent>
                  <EmptyState
                    icon={Check}
                    title="Nothing due right now"
                    hint="You're all caught up. Weak chords are surfaced here on their spaced-repetition schedule — come back when something is due."
                  />
                </CardContent>
              </Card>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
