import { Link } from "react-router-dom";
import { motion, type Variants } from "framer-motion";
import {
  ArrowUpRight,
  EyeOff,
  Lightbulb,
  Target,
} from "lucide-react";
import { toast } from "sonner";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { ProgressBar } from "@/components/ProgressBar";
import { WpmStatRow } from "@/components/WpmStatRow";
import { ActivityFeed } from "@/components/ActivityFeed";
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
import { useLiveSession } from "@/hooks/useLiveSession";
import { useHiddenWords } from "@/hooks/useHiddenWords";
import { formatNumber } from "@/lib/format";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

const stagger: Variants = {
  hidden: { opacity: 0, y: 12 },
  show: (i: number) => ({
    opacity: 1,
    y: 0,
    transition: { delay: i * 0.05, duration: 0.45, ease: [0.16, 1, 0.3, 1] },
  }),
};

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

export default function Dashboard() {
  const { blocks } = useLiveSession();
  const { data: suggestions, refresh: refreshSuggestions } = useSuggestions(12);
  const { data: proficiency } = useProficiency();
  const { hide } = useHiddenWords();

  const handleHideSuggestion = async (phrase: string) => {
    await hide(phrase);
    toast.success(`"${phrase}" hidden from suggestions.`);
    void refreshSuggestions();
  };

  // Glanceable secondary-screen view: show the top entries (panels scroll
  // internally). Full needs-practice lives on Proficiency; full suggestions on Words.
  const needsPractice = proficiency
    .filter((p) => !p.mastered)
    .sort((a, b) => {
      // Highest retype errors first, then deletions, then manual-usage, then fewest fires.
      if (b.error_rate !== a.error_rate) return b.error_rate - a.error_rate;
      if (b.deletion_rate !== a.deletion_rate) return b.deletion_rate - a.deletion_rate;
      if (a.usage_rate !== b.usage_rate) return a.usage_rate - b.usage_rate;
      return a.fired_count - b.fired_count;
    })
    .slice(0, 12);
  const topSuggestions = suggestions.slice(0, 12);
  // Only the single most-recent block. Full history lives on Analytics.
  const latestBlock = blocks.slice(0, 1);

  return (
    // Fill the routed content area so the four regions fit a 1440×900 secondary
    // screen with no scroll. Available height = 100vh − header(60px) −
    // main py-8(64px). Below the target size it scrolls gracefully via min-h.
    <div className="flex min-h-[calc(100vh-124px)] flex-col">
      <PageHeader
        title="Dashboard"
        subtitle="Your typing, distilled into mastery."
        className="mb-4"
      />

      {/* Compact WPM stat row — shared with Analytics so numbers stay in sync. */}
      <WpmStatRow compact />

      {/* Three equal panels side-by-side, each filling the remaining height so
          nothing gets pushed off-screen. Lists scroll internally only if they
          ever exceed their (capped) content. */}
      <div className="mt-4 grid min-h-0 flex-1 grid-cols-1 gap-4 lg:grid-cols-3">
        {/* Top suggestions */}
        <motion.div
          custom={1}
          initial="hidden"
          animate="show"
          variants={stagger}
          className="flex min-h-0"
        >
          <Card className="flex h-full w-full flex-col gap-2 py-3">
            <CardHeader className="flex-row items-center justify-between px-4">
              <CardTitle className="flex items-center gap-2 text-sm">
                <Lightbulb className="size-4 text-gold" /> Suggestions
              </CardTitle>
              <Button asChild variant="ghost" size="sm" className="h-7 text-muted-foreground">
                <Link to="/suggestions">
                  All <ArrowUpRight className="size-3.5" />
                </Link>
              </Button>
            </CardHeader>
            <CardContent className="min-h-0 flex-1 overflow-y-auto px-4">
              {topSuggestions.length ? (
                <TooltipProvider>
                  <ul className="divide-y divide-border">
                    {topSuggestions.map((s) => (
                      <li
                        key={s.phrase}
                        className="group flex items-center justify-between gap-3 py-1.5"
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

        {/* Needs practice — top 5 only; full list lives on Proficiency. */}
        <motion.div
          custom={2}
          initial="hidden"
          animate="show"
          variants={stagger}
          className="flex min-h-0"
        >
          <Card className="flex h-full w-full flex-col gap-2 py-3">
            <CardHeader className="flex-row items-center justify-between px-4">
              <CardTitle className="flex items-center gap-2 text-sm">
                <Target className="size-4 text-gold" /> Needs practice
              </CardTitle>
              <Button asChild variant="ghost" size="sm" className="h-7 text-muted-foreground">
                <Link to="/proficiency">
                  All <ArrowUpRight className="size-3.5" />
                </Link>
              </Button>
            </CardHeader>
            <CardContent className="min-h-0 flex-1 overflow-y-auto px-4">
              {needsPractice.length ? (
                <ul className="space-y-2.5">
                  {needsPractice.map((p) => {
                    // Show the most actionable reason this chord needs practice.
                    // Priority: retype errors > BS deletions > typed manually too often > not enough fires.
                    const label =
                      p.error_count > 0
                        ? `retype ${p.error_count}× · ${formatNumber(Math.round(p.error_rate * 100))}%`
                        : p.deletion_count > 0 && p.deletion_rate > 0.15
                          ? `del ${p.deletion_count}× · ${formatNumber(Math.round(p.deletion_rate * 100))}%`
                          : p.usage_rate < 0.7
                            ? `manual ${formatNumber(Math.round((1 - p.usage_rate) * 100))}% of the time`
                            : `only ${p.fired_count}× fired`;
                    const barValue =
                      p.error_count > 0 ? p.error_rate
                      : p.deletion_count > 0 && p.deletion_rate > 0.15 ? p.deletion_rate
                      : 1 - p.usage_rate;
                    const tone =
                      p.error_count > 0 ? (p.error_rate > 0.3 ? "danger" : "warning")
                      : p.deletion_count > 0 && p.deletion_rate > 0.15 ? "warning"
                      : "accent";
                    return (
                    <li key={p.phrase} className="space-y-1">
                      <div className="flex items-center justify-between gap-2 text-sm">
                        <span className="truncate font-mono text-foreground">{p.phrase}</span>
                        <span className="tnum shrink-0 text-xs text-muted-foreground">{label}</span>
                      </div>
                      <ProgressBar
                        value={barValue}
                        tone={tone}
                        size="sm"
                        aria-label={`${p.phrase} practice signal`}
                      />
                      <ComboLine combos={p.combos} />
                    </li>
                  );
                  })}
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

        {/* Latest activity — single most-recent block. */}
        <motion.div
          custom={3}
          initial="hidden"
          animate="show"
          variants={stagger}
          className="flex min-h-0"
        >
          <Card className="flex h-full w-full flex-col gap-2 py-3">
            <CardHeader className="flex-row items-center justify-between px-4">
              <CardTitle className="text-sm">Latest activity</CardTitle>
              <Button asChild variant="ghost" size="sm" className="h-7 text-muted-foreground">
                <Link to="/analytics">
                  Analytics <ArrowUpRight className="size-3.5" />
                </Link>
              </Button>
            </CardHeader>
            <CardContent className={cn("min-h-0 flex-1 space-y-2 overflow-y-auto px-4")}>
              <ActivityFeed
                blocks={latestBlock}
                bare
                emptyHint="Start typing — your most recent 5-minute window will appear here."
              />
            </CardContent>
          </Card>
        </motion.div>
      </div>
    </div>
  );
}
