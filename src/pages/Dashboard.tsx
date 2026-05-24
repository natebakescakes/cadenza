import { Link } from "react-router-dom";
import { motion, AnimatePresence, type Variants } from "framer-motion";
import {
  ArrowUpRight,
  EyeOff,
  Keyboard,
  Lightbulb,
  Target,
  Zap,
} from "lucide-react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { ProgressBar } from "@/components/ProgressBar";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useSuggestions } from "@/hooks/useSuggestions";
import { useProficiency } from "@/hooks/useProficiency";
import { useLiveSession, type LiveBlock } from "@/hooks/useLiveSession";
import { useHiddenWords } from "@/hooks/useHiddenWords";
import { formatNumber, formatWpm } from "@/lib/format";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

const BLOCK_MS = 5 * 60 * 1000;

const stagger: Variants = {
  hidden: { opacity: 0, y: 12 },
  show: (i: number) => ({
    opacity: 1,
    y: 0,
    transition: { delay: i * 0.05, duration: 0.45, ease: [0.16, 1, 0.3, 1] },
  }),
};

function blockLabel(blockStart: number): string {
  const start = new Date(blockStart);
  const end = new Date(blockStart + BLOCK_MS);
  const fmt = (d: Date) =>
    d.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  return `${fmt(start)} – ${fmt(end)}`;
}

/** Render one key-combination as small mono kbd boxes (e.g. "p + t"). */
function ComboKeys({ combo }: { combo: string }) {
  const keys = combo.split("+").map((k) => k.trim()).filter(Boolean);
  return (
    <span className="inline-flex flex-wrap items-center gap-1">
      {keys.map((key, i) => (
        <span key={`${key}-${i}`} className="inline-flex items-center gap-1">
          {i > 0 && (
            <span className="text-[10px] text-muted-foreground/50">+</span>
          )}
          <kbd className="inline-flex min-w-[1.1rem] items-center justify-center rounded border border-border bg-secondary/60 px-1 py-px font-mono text-[10px] leading-none text-foreground/80">
            {key}
          </kbd>
        </span>
      ))}
    </span>
  );
}

/** Combo reference line — quiet, secondary. Omits itself if no combos. */
function ComboLine({ combos }: { combos: string[] }) {
  if (!combos.length) return null;
  const multiple = combos.length > 1;
  return (
    <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-muted-foreground/70">
      <span className="shrink-0">press:</span>
      {combos.map((combo, i) => (
        <span key={`${combo}-${i}`} className="inline-flex items-center gap-1.5">
          {i > 0 && (
            <span className="text-muted-foreground/40">/</span>
          )}
          <ComboKeys combo={combo} />
        </span>
      ))}
      {multiple && (
        <span className="text-[10px] text-muted-foreground/50 italic">
          ({combos.length} duplicate chords)
        </span>
      )}
    </div>
  );
}

interface FoldedToken {
  text: string;
  source: "manual" | "chorded";
  count: number;
}

/**
 * Collapse runs of adjacent identical tokens (same text + source) into a
 * single entry carrying a count. Order is preserved; distinct tokens never
 * merge, so only consecutive repeats fold (e.g. "the the the" → the ×3).
 */
function foldRuns(words: string[], source: "manual" | "chorded"): FoldedToken[] {
  const out: FoldedToken[] = [];
  for (const text of words) {
    const last = out[out.length - 1];
    if (last && last.text === text && last.source === source) {
      last.count += 1;
    } else {
      out.push({ text, source, count: 1 });
    }
  }
  return out;
}

function WordChip({
  text,
  source,
  count,
}: {
  text: string;
  source: "manual" | "chorded";
  count: number;
}) {
  return (
    <motion.span
      layout
      initial={{ opacity: 0, scale: 0.88 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
      className={cn(
        "inline-flex items-center rounded-md border px-2 py-0.5 font-mono text-xs font-medium",
        source === "chorded"
          ? "border-info/30 bg-info/10 text-info"
          : "border-border bg-secondary/60 text-foreground/80",
      )}
    >
      {text}
      {source === "chorded" && (
        <Zap className="ml-1 size-2.5 shrink-0 opacity-70" />
      )}
      {count > 1 && (
        <span
          className={cn(
            "tnum ml-1 shrink-0 text-[10px] font-semibold tabular-nums",
            source === "chorded" ? "text-info/70" : "text-muted-foreground",
          )}
        >
          ×{count}
        </span>
      )}
    </motion.span>
  );
}

function BlockCard({
  block,
  isLatest,
}: {
  block: LiveBlock;
  isLatest: boolean;
}) {
  const allManual = [
    ...block.manualWords,
    ...block.liveEntries
      .filter((e) => e.source === "manual")
      .map((e) => e.text),
  ];
  const allChorded = [
    ...block.chorded_words,
    ...block.liveEntries
      .filter((e) => e.source === "chorded")
      .map((e) => e.text),
  ];
  const totalWords = allManual.length + allChorded.length;
  const foldedManual = foldRuns(allManual, "manual");
  const foldedChorded = foldRuns(allChorded, "chorded");

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }}
    >
      <Card className={cn("gap-0", isLatest && "ring-1 ring-gold/25")}>
        <CardHeader className="pb-2">
          <div className="flex items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              {isLatest && (
                <span className="size-1.5 rounded-full bg-success animate-pulse-soft" />
              )}
              <span className="text-xs font-medium text-muted-foreground">
                {blockLabel(block.blockStart)}
              </span>
            </div>
            <div className="flex items-center gap-1.5">
              {allManual.length > 0 && (
                <Badge
                  variant="outline"
                  className="tnum gap-1 px-1.5 py-0 text-[10px] text-muted-foreground"
                >
                  <Keyboard className="size-2.5" />
                  {allManual.length}
                </Badge>
              )}
              {allChorded.length > 0 && (
                <Badge
                  variant="outline"
                  className="tnum gap-1 px-1.5 py-0 text-[10px] text-info"
                >
                  <Zap className="size-2.5" />
                  {allChorded.length}
                </Badge>
              )}
              {block.wpm > 0 && (
                <Badge className="tnum bg-gold/15 text-gold border-gold/25 px-1.5 py-0 text-[10px] font-semibold">
                  {formatWpm(block.wpm)} wpm
                </Badge>
              )}
            </div>
          </div>
        </CardHeader>
        <CardContent>
          {totalWords === 0 ? (
            <p className="text-[11px] italic text-muted-foreground/50">
              No words yet.
            </p>
          ) : (
            <div className="flex flex-wrap gap-1">
              <AnimatePresence initial={false}>
                {foldedManual.map((t, i) => (
                  <WordChip
                    key={`m-${block.blockStart}-${i}-${t.text}`}
                    text={t.text}
                    source="manual"
                    count={t.count}
                  />
                ))}
                {foldedChorded.map((t, i) => (
                  <WordChip
                    key={`c-${block.blockStart}-${i}-${t.text}`}
                    text={t.text}
                    source="chorded"
                    count={t.count}
                  />
                ))}
              </AnimatePresence>
            </div>
          )}
        </CardContent>
      </Card>
    </motion.div>
  );
}

export default function Dashboard() {
  const { currentWpm, blocks } = useLiveSession();
  const { data: suggestions, refresh: refreshSuggestions } = useSuggestions(5);
  const { data: proficiency } = useProficiency();
  const { hide } = useHiddenWords();

  const handleHideSuggestion = async (phrase: string) => {
    await hide(phrase);
    toast.success(`"${phrase}" hidden from suggestions.`);
    void refreshSuggestions();
  };

  // Needs practice = used chords not yet mastered, sorted by error_rate desc
  // (backend already sorts). Mirrors the Proficiency page's "Practice" group so
  // the widget and full list agree. Cap at 4 for the dashboard panel.
  const needsPractice = proficiency.filter((p) => !p.mastered).slice(0, 4);
  const topSuggestions = suggestions.slice(0, 5);
  // Show at most 6 most-recent blocks on the dashboard
  const recentBlocks = blocks.slice(0, 6);
  const hasBlocks = recentBlocks.some(
    (b) =>
      b.manualWords.length > 0 ||
      b.chorded_words.length > 0 ||
      b.liveEntries.length > 0,
  );

  return (
    <div>
      <PageHeader
        title="Dashboard"
        subtitle="Your typing, distilled into mastery."
      />

      {/* Live WPM banner */}
      <motion.div custom={0} initial="hidden" animate="show" variants={stagger}>
        <Card className="mb-4 gap-0 py-0">
          <div className="flex items-center gap-4 px-5 py-4">
            <div className="grid size-9 shrink-0 place-items-center rounded-xl bg-gold/10">
              <Zap className="size-4 text-gold" strokeWidth={1.75} />
            </div>
            <div>
              <p className="text-[10px] font-medium tracking-wider text-muted-foreground/70 uppercase">
                Current pace (60 s window)
              </p>
              <div className="mt-0.5 flex items-baseline gap-1.5">
                <AnimatePresence mode="wait">
                  <motion.span
                    key={currentWpm ?? "null"}
                    initial={{ opacity: 0, y: 3 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: -3 }}
                    transition={{ duration: 0.25 }}
                    className="font-display tnum text-4xl font-semibold leading-none tracking-tight text-gold"
                  >
                    {currentWpm !== null ? formatWpm(currentWpm) : "—"}
                  </motion.span>
                </AnimatePresence>
                <span className="pb-0.5 text-sm font-medium text-muted-foreground">
                  wpm
                </span>
              </div>
            </div>
            <p className="ml-auto max-w-[15rem] text-right text-[11px] leading-relaxed text-balance text-muted-foreground/50">
              Words typed in the last 60 seconds, averaged as a per-minute rate.
            </p>
          </div>
        </Card>
      </motion.div>

      {/* Main grid: activity feed + side panels */}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        {/* Activity feed — 2 cols */}
        <motion.div
          custom={1}
          initial="hidden"
          animate="show"
          variants={stagger}
          className="lg:col-span-2"
        >
          <Card className="h-full">
            <CardHeader className="flex-row items-center justify-between pb-3">
              <CardTitle>Activity</CardTitle>
              <span className="text-xs text-muted-foreground">
                5-minute windows · last 24 h
              </span>
            </CardHeader>
            <CardContent className="space-y-2">
              {!hasBlocks ? (
                <EmptyState
                  icon={Zap}
                  title="No activity yet"
                  hint="Start typing — words and chords will appear here grouped into 5-minute windows."
                />
              ) : (
                <AnimatePresence initial={false}>
                  {recentBlocks.map((block, i) => (
                    <BlockCard
                      key={block.blockStart}
                      block={block}
                      isLatest={i === 0}
                    />
                  ))}
                </AnimatePresence>
              )}
            </CardContent>
          </Card>
        </motion.div>

        {/* Side column */}
        <div className="flex flex-col gap-4">
          {/* Top suggestions */}
          <motion.div custom={2} initial="hidden" animate="show" variants={stagger}>
            <Card>
              <CardHeader className="flex-row items-center justify-between">
                <CardTitle className="flex items-center gap-2">
                  <Lightbulb className="size-4 text-gold" /> Suggestions
                </CardTitle>
                <Button asChild variant="ghost" size="sm" className="text-muted-foreground">
                  <Link to="/suggestions">
                    All <ArrowUpRight className="size-3.5" />
                  </Link>
                </Button>
              </CardHeader>
              <CardContent>
                {topSuggestions.length ? (
                  <TooltipProvider>
                    <ul className="divide-y divide-border">
                      {topSuggestions.map((s) => (
                        <li
                          key={s.phrase}
                          className="group flex items-center justify-between gap-3 py-2"
                        >
                          <span className="font-mono text-sm text-foreground">
                            {s.phrase}
                          </span>
                          <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                            <span className="tnum">{formatNumber(s.frequency)}×</span>
                            <Badge variant="outline" className="tnum text-gold">
                              {Math.round(s.score)}
                            </Badge>
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <Button
                                  size="icon"
                                  variant="ghost"
                                  className="size-6 opacity-0 transition-opacity text-muted-foreground group-hover:opacity-100 hover:text-foreground focus-visible:opacity-100"
                                  aria-label={`Hide ${s.phrase}`}
                                  onClick={() => void handleHideSuggestion(s.phrase)}
                                >
                                  <EyeOff className="size-3" />
                                </Button>
                              </TooltipTrigger>
                              <TooltipContent side="left">Hide</TooltipContent>
                            </Tooltip>
                          </div>
                        </li>
                      ))}
                    </ul>
                  </TooltipProvider>
                ) : (
                  <EmptyState
                    compact
                    icon={Lightbulb}
                    title="No suggestions yet"
                    hint="Frequent hand-typed words appear here."
                  />
                )}
              </CardContent>
            </Card>
          </motion.div>

          {/* Needs practice */}
          <motion.div custom={3} initial="hidden" animate="show" variants={stagger}>
            <Card>
              <CardHeader className="flex-row items-center justify-between">
                <CardTitle className="flex items-center gap-2">
                  <Target className="size-4 text-gold" /> Needs practice
                </CardTitle>
                <Button asChild variant="ghost" size="sm" className="text-muted-foreground">
                  <Link to="/proficiency">
                    All <ArrowUpRight className="size-3.5" />
                  </Link>
                </Button>
              </CardHeader>
              <CardContent>
                {needsPractice.length ? (
                  <ul className="space-y-3">
                    {needsPractice.map((p) => (
                      <li key={p.phrase} className="space-y-1.5">
                        <div className="flex items-center justify-between text-sm">
                          <span className="font-mono text-foreground">{p.phrase}</span>
                          <span className="tnum text-xs text-muted-foreground">
                            {p.error_count > 0
                              ? `deleted ${p.error_count}×, ${formatNumber(Math.round(p.error_rate * 100))}% of the time`
                              : `used ${formatNumber(Math.round(p.usage_rate * 100))}%`}
                          </span>
                        </div>
                        <ProgressBar
                          value={p.error_count > 0 ? p.error_rate : p.usage_rate}
                          tone={p.error_count > 0 ? (p.error_rate > 0.3 ? "danger" : "warning") : "warning"}
                          size="sm"
                          aria-label={`${p.phrase} ${p.error_count > 0 ? "delete" : "usage"} rate`}
                        />
                        <ComboLine combos={p.combos} />
                      </li>
                    ))}
                  </ul>
                ) : (
                  <EmptyState
                    compact
                    icon={Target}
                    title="Nothing to practice"
                    hint="Chords to improve will appear here as you use your device."
                  />
                )}
              </CardContent>
            </Card>
          </motion.div>
        </div>
      </div>
    </div>
  );
}
