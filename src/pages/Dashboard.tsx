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
  const { data: suggestions, refresh: refreshSuggestions } = useSuggestions(5);
  const { data: proficiency } = useProficiency();
  const { hide } = useHiddenWords();

  const handleHideSuggestion = async (phrase: string) => {
    await hide(phrase);
    toast.success(`"${phrase}" hidden from suggestions.`);
    void refreshSuggestions();
  };

  // Needs practice = used chords not yet mastered, sorted by error_rate desc
  // (backend already sorts). Full list — the dashboard surfaces everything
  // that needs work so nothing is hidden behind a "view all".
  const needsPractice = proficiency.filter((p) => !p.mastered);
  const topSuggestions = suggestions.slice(0, 5);
  // Dashboard stays glanceable: only the single most-recent block. Full
  // history lives on Analytics.
  const latestBlock = blocks.slice(0, 1);

  return (
    <div>
      <PageHeader
        title="Dashboard"
        subtitle="Your typing, distilled into mastery."
      />

      {/* Full WPM stat row — shared with Analytics so numbers stay in sync. */}
      <WpmStatRow />

      {/* Main grid: side column leads with Needs practice; main holds latest
          activity + suggestions. */}
      <div className="mt-4 grid grid-cols-1 gap-4 lg:grid-cols-3">
        {/* Side column — Needs practice promoted to the top. */}
        <div className="order-1 flex flex-col gap-4 lg:order-2 lg:col-span-1">
          {/* Needs practice — primary panel, full list */}
          <motion.div custom={1} initial="hidden" animate="show" variants={stagger}>
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
        </div>

        {/* Latest activity — capped to the most recent window. */}
        <motion.div
          custom={3}
          initial="hidden"
          animate="show"
          variants={stagger}
          className="order-2 lg:order-1 lg:col-span-2"
        >
          <Card className="h-full">
            <CardHeader className="flex-row items-center justify-between pb-3">
              <CardTitle>Latest activity</CardTitle>
              <Button asChild variant="ghost" size="sm" className="text-muted-foreground">
                <Link to="/analytics">
                  View all in Analytics <ArrowUpRight className="size-3.5" />
                </Link>
              </Button>
            </CardHeader>
            <CardContent className={cn("space-y-2")}>
              <ActivityFeed
                blocks={latestBlock}
                emptyHint="Start typing — your most recent 5-minute window will appear here."
              />
            </CardContent>
          </Card>
        </motion.div>
      </div>
    </div>
  );
}
