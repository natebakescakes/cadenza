import { useCallback, useEffect, useRef, useState } from "react";
import { AnimatePresence, motion, type Variants } from "framer-motion";
import {
  Check,
  CornerDownLeft,
  Delete,
  Dumbbell,
  Eye,
  Flame,
  Gauge,
  Layers,
  Sparkles,
  Target,
  Timer,
  X,
  Zap,
} from "lucide-react";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { ComboKeys } from "@/components/ComboKeys";
import { FlowSession } from "@/components/FlowSession";
import { usePracticeGate } from "@/hooks/usePracticeGate";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  coachLog,
  practiceAllQueue,
  practiceCardStats,
  practiceCompleteSession,
  practiceDueQueue,
  practiceEnd,
  practiceOverview,
  practiceSessionSummary,
  practiceStartSession,
  practiceSubmitResult,
} from "@/lib/api";
import { formatMs, formatNumber, formatPercent } from "@/lib/format";
import type {
  PracticeAttemptSummary,
  PracticeCard,
  PracticeCardStats,
  PracticeOverview,
} from "@/lib/types";
import { cn } from "@/lib/utils";

const QUEUE_LIMIT = 30;
/** Delay before the chord (combo) hint is revealed for a card. Revealing it
 *  before the user completes the card discounts the rep below first-try credit. */
const HINT_DELAY_MS = 4000;
/** Min gap (ms) since the previous keystroke for an edit to count as a USER
 *  action. A compound chord/arpeggio emits its output (chars + corrective
 *  backspaces) as a synthetic burst <10ms apart; a human edit is far slower.
 *  Edits within this window are device output and must not count as fumbles. */
const BURST_MS = 80;

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
        {[0, 1, 2, 3, 4, 5, 6, 7].map((i) => (
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
      label: "Best time",
      value: stats.best_fire_ms > 0 ? formatMs(stats.best_fire_ms) : "—",
    },
    { label: "Clean rate", value: formatPercent(stats.clean_rate) },
    {
      label: "Avg backspaces",
      value: Number.isFinite(stats.avg_backspaces)
        ? stats.avg_backspaces.toFixed(1)
        : "—",
    },
    { label: "Hint rate", value: formatPercent(stats.hint_rate) },
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

// --- Post-session summary (per-word recap) --------------------------------

/** One aggregate cell in the summary header. */
function SummaryStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-border bg-secondary/40 px-3 py-2">
      <p className="text-[10px] tracking-wider text-muted-foreground/70 uppercase">
        {label}
      </p>
      <p className="tnum mt-0.5 text-sm font-semibold text-foreground">{value}</p>
    </div>
  );
}

/** A compact metric chip on a per-word row (icon + value). `tone` accents
 *  rows that need attention (amber = soft warning, red = error). */
function WordChip({
  icon: Icon,
  value,
  tone = "muted",
  title,
}: {
  icon: typeof Check;
  value: string;
  tone?: "muted" | "amber" | "red" | "success";
  title: string;
}) {
  return (
    <span
      title={title}
      className={cn(
        "inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[11px] font-medium tnum",
        tone === "muted" &&
          "border-border bg-secondary/40 text-muted-foreground/70",
        tone === "amber" && "border-gold/30 bg-gold/10 text-gold",
        tone === "red" &&
          "border-destructive/30 bg-destructive/10 text-destructive",
        tone === "success" && "border-success/30 bg-success/10 text-success",
      )}
    >
      <Icon className="size-3" strokeWidth={2} />
      {value}
    </span>
  );
}

/**
 * Post-session recap shown when a session ends. Header aggregates derived from
 * the per-attempt rows, then a scannable per-word list with slow/incorrect/
 * hinted words visually flagged so the user can spot what to work on.
 */
function SummaryPanel({
  rows,
  loading,
  onAgain,
}: {
  rows: PracticeAttemptSummary[] | null;
  loading: boolean;
  onAgain: () => void;
}) {
  if (loading) {
    return (
      <div className="space-y-2">
        {[0, 1, 2, 3].map((i) => (
          <div
            key={i}
            className="h-12 animate-pulse rounded-lg border border-border bg-secondary/40"
          />
        ))}
      </div>
    );
  }

  if (!rows || rows.length === 0) {
    return (
      <Card className="flex flex-1 items-center justify-center">
        <CardContent className="flex flex-col items-center gap-4 py-10 text-center">
          <EmptyState
            icon={Check}
            title="No attempts recorded"
            hint="This session didn't log any graded words."
          />
          <Button onClick={onAgain}>
            <Dumbbell className="size-4" />
            Practice again
          </Button>
        </CardContent>
      </Card>
    );
  }

  const total = rows.length;
  const correctCount = rows.filter((r) => r.correct).length;
  const firstTryCount = rows.filter((r) => r.first_try).length;
  const correctRows = rows.filter((r) => r.correct);
  const avgFireMs =
    correctRows.length > 0
      ? correctRows.reduce((s, r) => s + r.fire_ms, 0) / correctRows.length
      : 0;
  const totalBackspaces = rows.reduce((s, r) => s + r.backspaces, 0);
  const totalCorrections = rows.reduce((s, r) => s + r.corrections, 0);
  const hintCount = rows.filter((r) => r.hint_used).length;

  // Slow = noticeably above the session's mean correct time (used to flag rows).
  const slowThreshold = avgFireMs > 0 ? avgFireMs * 1.5 : Infinity;

  return (
    <div className="flex flex-1 flex-col gap-5">
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-7">
        <SummaryStat label="Words" value={formatNumber(total)} />
        <SummaryStat label="Accuracy" value={formatPercent(correctCount / total)} />
        <SummaryStat label="First-try" value={formatPercent(firstTryCount / total)} />
        <SummaryStat label="Avg time" value={formatMs(avgFireMs)} />
        <SummaryStat label="Backspaces" value={formatNumber(totalBackspaces)} />
        <SummaryStat label="Corrections" value={formatNumber(totalCorrections)} />
        <SummaryStat label="Hints" value={formatNumber(hintCount)} />
      </div>

      <div className="flex flex-1 flex-col gap-1.5 overflow-y-auto pr-1">
        {rows.map((r, i) => {
          const slow = r.correct && r.fire_ms > slowThreshold;
          const needsWork = !r.correct || r.hint_used || slow;
          return (
            <div
              key={`${r.phrase}-${r.ts}-${i}`}
              className={cn(
                "flex items-center gap-3 rounded-lg border px-3 py-2 transition-colors",
                r.correct
                  ? needsWork
                    ? "border-gold/25 bg-gold/[0.04]"
                    : "border-border bg-secondary/30"
                  : "border-destructive/30 bg-destructive/[0.06]",
              )}
            >
              {r.correct ? (
                <Check
                  className={cn(
                    "size-4 shrink-0",
                    needsWork ? "text-gold" : "text-success/70",
                  )}
                  strokeWidth={2.2}
                />
              ) : (
                <X className="size-4 shrink-0 text-destructive" strokeWidth={2.2} />
              )}
              <span className="min-w-0 flex-1 truncate font-mono text-sm font-medium text-foreground">
                {r.phrase}
              </span>
              <div className="flex shrink-0 flex-wrap items-center justify-end gap-1.5">
                <WordChip
                  icon={Timer}
                  value={formatMs(r.fire_ms)}
                  tone={slow ? "amber" : "muted"}
                  title="Time to complete"
                />
                {r.first_try ? (
                  <WordChip
                    icon={Check}
                    value="1st"
                    tone="success"
                    title="First-try correct"
                  />
                ) : (
                  <WordChip
                    icon={CornerDownLeft}
                    value="retry"
                    tone="amber"
                    title="Not first-try"
                  />
                )}
                {r.backspaces > 0 && (
                  <WordChip
                    icon={Delete}
                    value={String(r.backspaces)}
                    tone="amber"
                    title="Backspaces"
                  />
                )}
                {r.corrections > 0 && (
                  <WordChip
                    icon={CornerDownLeft}
                    value={String(r.corrections)}
                    tone="amber"
                    title="Corrections"
                  />
                )}
                {r.hint_used && (
                  <WordChip
                    icon={Eye}
                    value="hint"
                    tone="amber"
                    title="Hint revealed"
                  />
                )}
              </div>
            </div>
          );
        })}
      </div>

      <div className="flex justify-end">
        <Button onClick={onAgain}>
          <Dumbbell className="size-4" />
          Practice again
        </Button>
      </div>
    </div>
  );
}

// --- Page -----------------------------------------------------------------

type Phase = "idle" | "drilling" | "done";
/** Recall = cold-recall, one card at a time. Flow = look-ahead, continuous line. */
type PracticeMode = "recall" | "flow";
/** Queue source: "due" = spaced-repetition due + weak chords; "all" = a random
 *  sample of the whole device chord library. Only swaps WHICH cards drill —
 *  grading + SR submission downstream are identical for both. */
type QueueSource = "due" | "all";

export default function Practice() {
  const [queue, setQueue] = useState<PracticeCard[]>([]);
  const [overview, setOverview] = useState<PracticeOverview | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  // Chosen drill mode (component-state only; not persisted across reloads).
  const [mode, setMode] = useState<PracticeMode>("recall");
  // Where the queue is sourced from (component-state only; not persisted).
  const [source, setSource] = useState<QueueSource>("due");
  const [index, setIndex] = useState(0);
  const [feedback, setFeedback] = useState<Feedback>(null);
  const [cardStats, setCardStats] = useState<PracticeCardStats | null>(null);
  const [loading, setLoading] = useState(true);
  // The just-completed session id, captured before any reset so the done view
  // can fetch its per-word recap. null = no summary to show.
  const [completedSessionId, setCompletedSessionId] = useState<number | null>(null);
  const [summary, setSummary] = useState<PracticeAttemptSummary[] | null>(null);
  const [summaryLoading, setSummaryLoading] = useState(false);
  // Whether the drill input holds keyboard focus. When false we dim + steer the
  // user back: the practice gate (which suppresses ambient stats + coaching) is
  // tied to this focus, so blurring turns the gate OFF (coaching resumes) and
  // refocusing turns it back ON.
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
  // Wall-clock start (performance.now) of the active card; used for fireMs.
  const cardStartRef = useRef(0);
  // Timestamp of the previous input event, to tell device bursts from user edits.
  const lastInputTsRef = useRef(0);
  // True once the user backspaces or types a non-prefix (wrong) char this card.
  const hadCorrectionRef = useRef(false);
  // Per-card edit counters (reset in beginCard).
  const backspacesRef = useRef(0);
  const correctionsRef = useRef(0);
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
    const fetchQueue =
      source === "all"
        ? practiceAllQueue(QUEUE_LIMIT)
        : practiceDueQueue(QUEUE_LIMIT);
    void fetchQueue
      .then((cards) => setQueue(cards))
      .catch(() => setQueue([]))
      .finally(() => setLoading(false));
  }, [source]);

  // Load the queue + overview once on mount, and reload the queue whenever the
  // source changes (so the idle list + count reflect the choice). (The queue is
  // a spaced-repetition set that changes slowly; it's also reloaded after a
  // session ends. We do NOT refetch on every window focus — that re-ran the
  // heavy proficiency query and flashed the UI every time the window regained
  // focus.)
  useEffect(() => {
    loadQueue();
    refreshOverview();
  }, [loadQueue, refreshOverview]);

  // Fetch the per-word recap when a session completes (both modes hand the
  // just-finished session id here). Cancelled if a newer session supersedes it.
  useEffect(() => {
    if (completedSessionId == null) {
      setSummary(null);
      return;
    }
    let cancelled = false;
    setSummaryLoading(true);
    void practiceSessionSummary(completedSessionId)
      .then((rows) => {
        if (!cancelled) setSummary(rows);
      })
      .catch(() => {
        if (!cancelled) setSummary([]);
      })
      .finally(() => {
        if (!cancelled) setSummaryLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [completedSessionId]);

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
      // The practice gate is released by usePracticeGate's own unmount cleanup.
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
    backspacesRef.current = 0;
    correctionsRef.current = 0;
    cardStartRef.current = performance.now();
    lastInputTsRef.current = performance.now();
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
      const sid = sessionIdRef.current;
      // Capture the completed session id for the recap BEFORE any reset.
      setCompletedSessionId(sid);
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

  // Gate driven by input AND window focus (so switching apps releases it and
  // coaching resumes). `focused` reflects the genuine active state.
  const { active: focused, onInputFocus, onInputBlur } = usePracticeGate(currentPhrase);

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
      // Only count an edit as a USER correction if it's slow enough to be human.
      // A compound chord/arpeggio emits chars + corrective backspaces as a rapid
      // synthetic burst (its output, not a fumble), so we ignore burst edits.
      const now = performance.now();
      const userEdit = now - lastInputTsRef.current >= BURST_MS;
      lastInputTsRef.current = now;
      if (next.length < prevLen) {
        if (userEdit) {
          backspacesRef.current += 1;
          hadCorrectionRef.current = true;
        }
      } else if (userEdit && trimmed.length > 0 && !targetTrimmed.startsWith(trimmed)) {
        correctionsRef.current += 1;
        hadCorrectionRef.current = true;
      }
      setValue(next);

      if (trimmed === targetTrimmed) {
        const fireMs = Math.max(0, Math.round(performance.now() - cardStartRef.current));
        // First-try gated on the hint only (not backspaces/corrections): an
        // arpeggio rolls through transient non-matches + device backspaces
        // before settling correct, so those must not count as a fumble. Raw
        // counts are still recorded for the summary.
        const firstTry = !hintShownRef.current;
        if (hintTimerRef.current != null) {
          window.clearTimeout(hintTimerRef.current);
          hintTimerRef.current = null;
        }
        const sid = sessionIdRef.current;
        if (sid != null) {
          void practiceSubmitResult(
            sid,
            target,
            true,
            firstTry,
            fireMs,
            backspacesRef.current,
            correctionsRef.current,
            hintShownRef.current,
          ).catch(() => undefined);
        }
        setFeedback({ correct: true, fireMs, firstTry });
        // Live header tick: this rep is logged, so refresh the overview now.
        refreshOverview();
        // Brief beat to show the verdict, then advance.
        window.setTimeout(() => advanceRef.current(), 650);
      }
    },
    [feedback, value, refreshOverview],
  );

  const startSession = useCallback(async () => {
    if (!queue.length) return;
    // Leaving the done/recap view for a fresh drill: clear the prior summary.
    setCompletedSessionId(null);
    // Flow runs a self-contained session inside <FlowSession/> (its own
    // start/complete + focus gate). Just enter the drilling phase for it.
    if (mode === "flow") {
      setIndex(0);
      setPhase("drilling");
      return;
    }
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
  }, [queue, mode, beginCard, focusInput]);

  const quitSession = useCallback(() => {
    void coachLog("[PRACTICE-FE] end reason=quit");
    if (hintTimerRef.current != null) {
      window.clearTimeout(hintTimerRef.current);
      hintTimerRef.current = null;
    }
    inPracticeRef.current = false;
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

  // Flow runs its own session/gate internally; the parent only reacts to its
  // terminal events. Quit -> back to the queue; complete -> done + refresh.
  const flowQuit = useCallback(() => {
    setPhase("idle");
    refreshOverview();
    loadQueue();
  }, [refreshOverview, loadQueue]);

  const flowComplete = useCallback(
    (sid: number) => {
      setCompletedSessionId(sid);
      setPhase("done");
      refreshOverview();
      loadQueue();
    },
    [refreshOverview, loadQueue],
  );

  // "Practice again": leave the recap and return to the idle/queue view.
  const returnToQueue = useCallback(() => {
    setCompletedSessionId(null);
    setSummary(null);
    setPhase("idle");
  }, []);

  const currentCard = phase === "drilling" ? queue[index] : undefined;

  return (
    <div className="flex min-h-[calc(100vh-124px)] flex-col">
      <PageHeader
        title="Practice"
        subtitle="Drill your weakest chords — spaced repetition, one at a time."
        actions={
          phase === "drilling" ? (
            <Button
              variant="ghost"
              size="sm"
              onClick={mode === "flow" ? flowQuit : quitSession}
            >
              End session
            </Button>
          ) : undefined
        }
      />

      <OverviewBar overview={overview} />

      <AnimatePresence mode="wait">
        {phase === "drilling" && mode === "flow" ? (
          <FlowSession
            key="flow"
            queue={queue}
            onQuit={flowQuit}
            onComplete={flowComplete}
            onRepComplete={refreshOverview}
          />
        ) : phase === "drilling" && currentCard ? (
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
                  onFocus={onInputFocus}
                  onBlur={onInputBlur}
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
        ) : phase === "done" && completedSessionId != null ? (
          <motion.div
            key="summary"
            initial={{ opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -8 }}
            transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }}
            className="mt-6 flex flex-1 flex-col"
          >
            <div className="mb-4 flex items-center gap-2">
              <Gauge className="size-4 text-gold" />
              <h2 className="text-sm font-medium text-foreground">
                Session recap
              </h2>
            </div>
            <SummaryPanel
              rows={summary}
              loading={summaryLoading}
              onAgain={returnToQueue}
            />
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
            <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
              <div className="flex items-center gap-2">
                <Gauge className="size-4 text-gold" />
                <h2 className="text-sm font-medium text-foreground">
                  {source === "all" ? "Whole library" : "Due now"}
                </h2>
                <Badge variant="outline" className="tnum text-muted-foreground">
                  {queue.length}
                </Badge>
              </div>
              <div className="flex flex-wrap items-center gap-3">
                {/* Queue source: Due (SR due + weak) vs Whole library (random sample). */}
                <div
                  role="radiogroup"
                  aria-label="Queue source"
                  className="inline-flex rounded-lg border border-border bg-secondary/40 p-0.5"
                >
                  {(
                    [
                      { value: "due", label: "Due" },
                      { value: "all", label: "Whole library" },
                    ] as const
                  ).map((s) => (
                    <button
                      key={s.value}
                      type="button"
                      role="radio"
                      aria-checked={source === s.value}
                      onClick={() => setSource(s.value)}
                      className={cn(
                        "rounded-md px-3 py-1 text-xs font-medium transition-colors",
                        source === s.value
                          ? "bg-background text-foreground shadow-sm"
                          : "text-muted-foreground/70 hover:text-foreground",
                      )}
                    >
                      {s.label}
                    </button>
                  ))}
                </div>
                {/* Mode selector: Recall (cold recall) vs Flow (look-ahead). */}
                <div
                  role="radiogroup"
                  aria-label="Practice mode"
                  className="inline-flex rounded-lg border border-border bg-secondary/40 p-0.5"
                >
                  {(["recall", "flow"] as const).map((m) => (
                    <button
                      key={m}
                      type="button"
                      role="radio"
                      aria-checked={mode === m}
                      onClick={() => setMode(m)}
                      className={cn(
                        "rounded-md px-3 py-1 text-xs font-medium capitalize transition-colors",
                        mode === m
                          ? "bg-background text-foreground shadow-sm"
                          : "text-muted-foreground/70 hover:text-foreground",
                      )}
                    >
                      {m}
                    </button>
                  ))}
                </div>
                <Button
                  onClick={() => void startSession()}
                  disabled={!queue.length}
                >
                  <Dumbbell className="size-4" />
                  {phase === "done" ? "Practice again" : "Start session"}
                </Button>
              </div>
            </div>

            <p className="mb-4 text-xs text-muted-foreground/70">
              {source === "all"
                ? "Whole library — a random sample of all your chords (re-sampled each session)."
                : "Due — spaced-repetition due cards plus your weak chords."}
            </p>

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
