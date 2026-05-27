import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion, type Variants } from "framer-motion";
import {
  Check,
  Dumbbell,
  Flame,
  Gauge,
  Layers,
  Sparkles,
  Target,
  X,
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
  onPracticeChord,
} from "@/lib/api";
import { formatMs, formatNumber, formatPercent } from "@/lib/format";
import type {
  PracticeCard,
  PracticeCardStats,
  PracticeOverview,
} from "@/lib/types";
import { cn } from "@/lib/utils";

const QUEUE_LIMIT = 30;

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
  // Whether the drill surface holds keyboard focus. When false we dim + warn:
  // the chord still fires (detection is global via the keylogger) but its
  // keystroke OUTPUT lands in whatever app is focused, so we steer the user to
  // keep Cadenza focused and we swallow that output here (onKeyDown below).
  const [focused, setFocused] = useState(true);
  const drillRef = useRef<HTMLDivElement>(null);
  const focusDrill = useCallback(() => {
    // Defer so the element exists after the phase/card render.
    requestAnimationFrame(() => drillRef.current?.focus());
  }, []);

  // Drill state held in refs so the long-lived event listener always reads
  // current values without needing to re-subscribe per card.
  const sessionIdRef = useRef<number | null>(null);
  const drillQueueRef = useRef<PracticeCard[]>([]);
  const indexRef = useRef(0);
  // True once the active prompt has seen an incorrect/failed chord — kills first_try.
  const failedRef = useRef(false);
  const phaseRef = useRef<Phase>("idle");
  // Guard so practice_end runs exactly once on leave.
  const inPracticeRef = useRef(false);

  phaseRef.current = phase;

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

  // Always leave practice mode when the page unmounts.
  useEffect(() => {
    return () => {
      if (inPracticeRef.current) {
        void coachLog("[PRACTICE-FE] end reason=unmount");
        inPracticeRef.current = false;
        const sid = sessionIdRef.current;
        if (sid != null) void practiceCompleteSession(sid).catch(() => undefined);
        void practiceEnd().catch(() => undefined);
      }
    };
  }, []);

  const loadStatsFor = useCallback((phrase: string) => {
    setCardStats(null);
    void practiceCardStats(phrase)
      .then(setCardStats)
      .catch(() => setCardStats(null));
  }, []);

  // Move to the next card or finish the session. Refs keep the listener in sync.
  const advance = useCallback(() => {
    const next = indexRef.current + 1;
    const cards = drillQueueRef.current;
    if (next >= cards.length) {
      // End of session.
      void coachLog(`[PRACTICE-FE] end reason=queue-complete count=${cards.length}`);
      inPracticeRef.current = false;
      const sid = sessionIdRef.current;
      if (sid != null) void practiceCompleteSession(sid).catch(() => undefined);
      void practiceEnd().catch(() => undefined);
      setPhase("done");
      setFeedback(null);
      refreshOverview();
      loadQueue();
      return;
    }
    indexRef.current = next;
    failedRef.current = false;
    setIndex(next);
    setFeedback(null);
    const phrase = cards[next].phrase;
    loadStatsFor(phrase);
    void practiceBegin(phrase).catch(() => undefined);
    focusDrill();
  }, [loadQueue, loadStatsFor, refreshOverview, focusDrill]);

  const advanceRef = useRef(advance);
  advanceRef.current = advance;

  // Single subscription for the whole drill lifetime. It reads refs so it never
  // goes stale across card transitions.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void onPracticeChord((e) => {
      if (phaseRef.current !== "drilling") return;
      const cards = drillQueueRef.current;
      const current = cards[indexRef.current];
      if (!current || e.phrase !== current.phrase) return;

      if (!e.correct) {
        // A wrong chord for this prompt disqualifies first_try; keep waiting.
        failedRef.current = true;
        setFeedback({ correct: false, fireMs: e.fire_ms, firstTry: false });
        const sid = sessionIdRef.current;
        if (sid != null) {
          void practiceSubmitResult(
            sid,
            current.phrase,
            false,
            false,
            e.fire_ms,
          ).catch(() => undefined);
        }
        return;
      }

      // Correct chord: first_try is true only if no prior failure on this prompt.
      const firstTry = !failedRef.current;
      const sid = sessionIdRef.current;
      if (sid != null) {
        void practiceSubmitResult(
          sid,
          current.phrase,
          true,
          firstTry,
          e.fire_ms,
        ).catch(() => undefined);
      }
      setFeedback({ correct: true, fireMs: e.fire_ms, firstTry });
      // Brief beat to show the verdict, then advance.
      window.setTimeout(() => {
        if (!cancelled) advanceRef.current();
      }, 650);
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const startSession = useCallback(async () => {
    if (!queue.length) return;
    const cards = queue;
    try {
      const sid = await practiceStartSession();
      sessionIdRef.current = sid;
      drillQueueRef.current = cards;
      indexRef.current = 0;
      failedRef.current = false;
      inPracticeRef.current = true;
      setIndex(0);
      setFeedback(null);
      setPhase("drilling");
      loadStatsFor(cards[0].phrase);
      await practiceBegin(cards[0].phrase);
      void coachLog(`[PRACTICE-FE] session start queue=${cards.length}`);
      focusDrill();
    } catch {
      // If we couldn't enter practice mode, fall back to the queue view.
      inPracticeRef.current = false;
      setPhase("idle");
    }
  }, [queue, loadStatsFor, focusDrill]);

  const quitSession = useCallback(() => {
    void coachLog("[PRACTICE-FE] end reason=quit");
    inPracticeRef.current = false;
    const sid = sessionIdRef.current;
    if (sid != null) void practiceCompleteSession(sid).catch(() => undefined);
    void practiceEnd().catch(() => undefined);
    sessionIdRef.current = null;
    setPhase("idle");
    setFeedback(null);
    refreshOverview();
    loadQueue();
  }, [loadQueue, refreshOverview]);

  const currentCard = useMemo(
    () => (phase === "drilling" ? queue[index] : undefined),
    [phase, queue, index],
  );

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
            ref={drillRef}
            tabIndex={0}
            onFocus={() => setFocused(true)}
            onBlur={() => setFocused(false)}
            onKeyDown={(e) => {
              // Escape quits; let Tab move focus normally. Everything else is the
              // chord's keystroke OUTPUT landing in the webview — swallow it so it
              // can't trigger page actions/navigation (which previously unmounted
              // the page and ended the session mid-drill). Detection itself is
              // independent (global keylogger), so swallowing here is safe.
              if (e.key === "Escape") {
                quitSession();
                return;
              }
              if (e.key === "Tab") return;
              e.preventDefault();
            }}
            initial={{ opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -8 }}
            transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }}
            className="relative mt-6 flex flex-1 flex-col outline-none"
          >
            {!focused && (
              <button
                type="button"
                onClick={() => drillRef.current?.focus()}
                className="absolute inset-0 z-10 flex flex-col items-center justify-center gap-1 rounded-xl bg-background/70 text-center backdrop-blur-sm"
              >
                <span className="text-sm font-medium text-foreground">Click to focus</span>
                <span className="text-xs text-muted-foreground">
                  Chords type into the focused app — keep Cadenza focused while drilling.
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
                  Chord this
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

                {currentCard.combos.length > 0 && (
                  <div className="flex flex-col items-center gap-1.5">
                    {currentCard.combos.map((combo, i) => (
                      <ComboKeys key={`${combo}-${i}`} combo={combo} />
                    ))}
                  </div>
                )}

                <div className="flex h-8 items-center">
                  <AnimatePresence mode="wait">
                    {feedback && (
                      <motion.div
                        key={`${feedback.correct}-${feedback.fireMs}`}
                        initial={{ opacity: 0, y: 6 }}
                        animate={{ opacity: 1, y: 0 }}
                        exit={{ opacity: 0, y: -6 }}
                        transition={{ duration: 0.2 }}
                        className={cn(
                          "inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs font-medium",
                          feedback.correct
                            ? "border-success/30 bg-success/10 text-success"
                            : "border-danger/30 bg-danger/10 text-danger",
                        )}
                      >
                        {feedback.correct ? (
                          <>
                            <Check className="size-3.5" />
                            {feedback.firstTry ? "Nice" : "Got it"} ·{" "}
                            {formatMs(feedback.fireMs)}
                          </>
                        ) : (
                          <>
                            <X className="size-3.5" /> Try again
                          </>
                        )}
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
