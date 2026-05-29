import { useMemo, useState } from "react";
import { motion } from "framer-motion";
import {
  CheckCircle2,
  Gauge,
  Info,
  Target,
} from "lucide-react";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { ProgressBar } from "@/components/ProgressBar";
import { ComboKeys } from "@/components/ComboKeys";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useProficiency } from "@/hooks/useProficiency";
import { formatMs, formatPercent } from "@/lib/format";
import type { Proficiency as Prof } from "@/lib/types";

type Filter = "all" | "mastered" | "practice";

/** Weighted score: usage 55%, consistency 25%, no-confusion 15%, retype accuracy 5%.
 *  Deletion rate excluded: pass-through backspaces scapegoat simple high-frequency chords. */
function profScore(p: Prof): number {
  return (
    p.usage_rate * 0.55 +
    p.consistency * 0.25 +
    (1 - p.confusion_rate) * 0.15 +
    (1 - p.error_rate) * 0.05
  );
}

/** Combo reference block for a chord card. Quiet/secondary; omits if empty. */
function ComboBlock({ combos }: { combos: string[] }) {
  if (!combos.length) return null;
  const multiple = combos.length > 1;
  return (
    <div className="space-y-1.5 border-t border-border pt-2.5">
      <p className="text-[10px] tracking-wider text-muted-foreground/70 uppercase">
        {multiple ? "Key combos" : "Key combo"}
      </p>
      <div className="flex flex-col gap-1.5">
        {combos.map((combo, i) => (
          <div key={`${combo}-${i}`} className="flex items-center gap-2">
            <ComboKeys combo={combo} />
            {multiple && i > 0 && (
              <span className="text-[10px] text-muted-foreground/50 italic">
                alternate
              </span>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function ParamRow({
  label,
  value,
  bar,
  tone,
  hint,
  inverted = false,
  dimWhenZero = false,
}: {
  label: string;
  value: string;
  bar: number;
  tone: "success" | "warning" | "danger" | "accent";
  hint?: string;
  inverted?: boolean;
  dimWhenZero?: boolean;
}) {
  const isEmpty = dimWhenZero && bar === 0;
  return (
    <div className={isEmpty ? "opacity-40" : ""}>
      <div className="flex items-center justify-between text-xs">
        <span className="flex items-center gap-1 text-muted-foreground">
          {label}
          {hint && (
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Info className="size-3 opacity-50 cursor-help" />
                </TooltipTrigger>
                <TooltipContent side="top" className="max-w-[220px] text-left text-[11px]">
                  {hint}
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
          )}
        </span>
        <span className="tnum font-medium text-foreground">{value}</span>
      </div>
      <ProgressBar
        value={inverted ? 1 - bar : bar}
        tone={tone}
        size="sm"
        aria-label={label}
      />
    </div>
  );
}

function ProfCard({ p }: { p: Prof }) {
  const score = profScore(p);
  const scoreTone = score >= 0.75 ? "success" : score >= 0.45 ? "warning" : "danger";

  return (
    <motion.div
      layout
      initial={{ opacity: 0, scale: 0.97 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }}
    >
      <Card className="gap-3 py-4 transition-colors hover:ring-foreground/20">
        <CardContent className="space-y-3">
          {/* Header */}
          <div className="flex items-center justify-between gap-2">
            <span className="font-mono text-sm font-medium text-foreground">{p.phrase}</span>
            <Badge
              variant={p.mastered ? "default" : "outline"}
              className={p.mastered ? "bg-success/15 text-success border-success/30" : "text-muted-foreground"}
            >
              {p.mastered ? "Mastered" : "Practice"}
            </Badge>
          </div>

          {/* Weighted score */}
          <div className="space-y-1">
            <div className="flex items-center justify-between text-xs">
              <span className="text-muted-foreground">Score</span>
              <span className="tnum font-semibold text-foreground">{formatPercent(score)}</span>
            </div>
            <ProgressBar value={score} tone={scoreTone} size="sm" aria-label={`${p.phrase} score`} />
          </div>

          {/* Raw parameters */}
          <div className="space-y-2 pt-0.5">
            <ParamRow
              label="Usage"
              value={`${formatPercent(p.usage_rate)} (${p.fired_count}× fired)`}
              bar={p.usage_rate}
              tone="accent"
              hint="Chord fires ÷ total occurrences (fired + manual). High = you rely on the chord."
            />
            <ParamRow
              label="Retype errors"
              value={p.error_count > 0 ? `${formatPercent(p.error_rate)} (${p.error_count}×)` : "none"}
              bar={p.error_rate}
              tone="danger"
              inverted
              dimWhenZero
              hint="High-confidence: chord fired then same phrase manually retyped within 5s."
            />
            <ParamRow
              label="Deletions"
              value={p.deletion_count > 0 ? `${formatPercent(p.deletion_rate)} (${p.deletion_count}×)` : "none"}
              bar={p.deletion_rate}
              tone="warning"
              inverted
              dimWhenZero
              hint="Lower-confidence: chord output deleted by backspace within 3s. May include intentional edits."
            />
            <ParamRow
              label="Confusions"
              value={p.confusion_count > 0 ? `${formatPercent(p.confusion_rate)} (${p.confusion_count}×)` : "none"}
              bar={p.confusion_rate}
              tone="danger"
              inverted
              dimWhenZero
              hint="Chord deleted then a different chord fired within the confusion window — indicates chord mix-up."
            />
            <ParamRow
              label="Consistency"
              value={formatPercent(p.consistency)}
              bar={p.consistency}
              tone="accent"
              hint="Confidence from repetition — rises toward 100% as you fire the chord more."
            />
          </div>

          {/* Stats */}
          <div className="grid grid-cols-2 gap-2 pt-1">
            <div className="rounded-lg border border-border bg-secondary/40 px-2.5 py-2">
              <p className="text-[10px] tracking-wider text-muted-foreground/70 uppercase">Fire speed</p>
              <p className="tnum mt-0.5 text-sm font-semibold text-foreground">
                {p.avg_fire_ms > 0 ? formatMs(p.avg_fire_ms) : "—"}
              </p>
            </div>
            <div className="rounded-lg border border-border bg-secondary/40 px-2.5 py-2">
              <p className="text-[10px] tracking-wider text-muted-foreground/70 uppercase">Manual</p>
              <p className="tnum mt-0.5 text-sm font-semibold text-foreground">{p.manual_count}×</p>
            </div>
          </div>

          <ComboBlock combos={p.combos} />
        </CardContent>
      </Card>
    </motion.div>
  );
}

export default function Proficiency() {
  const { data, loading } = useProficiency();
  const [filter, setFilter] = useState<Filter>("all");

  const { mastered, practice } = useMemo(() => {
    return {
      mastered: data.filter((p) => p.mastered),
      practice: data.filter(
        (p) => !p.mastered && (p.error_count > 0 || p.confusion_count > 0 || p.usage_rate < 0.5)
      ),
    };
  }, [data]);

  const showMastered = filter === "all" || filter === "mastered";
  const showPractice = filter === "all" || filter === "practice";

  return (
    <div>
      <PageHeader
        title="Proficiency"
        subtitle="Are you actually firing your chords — fast and consistently?"
        actions={
          <div className="flex items-center gap-2">
            <TooltipProvider>
              <Tooltip>
                <TooltipTrigger asChild>
                  <button
                    aria-label="How proficiency is scored"
                    className="grid size-8 place-items-center rounded-lg text-muted-foreground outline-none transition-colors hover:bg-secondary hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50"
                  >
                    <Info className="size-4" />
                  </button>
                </TooltipTrigger>
                <TooltipContent side="bottom" className="max-w-[280px] text-left">
                  Score = usage 55% + consistency 25% + no-confusion 15% + retype accuracy 5%. Deletion rate shown for reference only — too noisy to score reliably.{" "}
                  <span className="text-success">Mastered</span>: ≈15+ fires (consistency ≥75%), &lt;10% retypes, &lt;10% confusions, chorded ≥80% of occurrences.{" "}
                  <span className="text-gold">Practice</span>: used but not yet reliable.
                </TooltipContent>
              </Tooltip>
            </TooltipProvider>
            <Tabs value={filter} onValueChange={(v) => setFilter(v as Filter)}>
              <TabsList>
                <TabsTrigger value="all">All</TabsTrigger>
                <TabsTrigger value="mastered">Mastered</TabsTrigger>
                <TabsTrigger value="practice">Practice</TabsTrigger>
              </TabsList>
            </Tabs>
          </div>
        }
      />

      {loading && data.length === 0 ? (
        <Card>
          <CardContent>
            <EmptyState
              icon={Gauge}
              title="Crunching your chord stats…"
              hint="Comparing how often you fire each chord versus typing it out by hand."
            />
          </CardContent>
        </Card>
      ) : data.length === 0 ? (
        <Card>
          <CardContent>
            <EmptyState
              icon={Target}
              title="No proficiency data yet"
              hint="Connect your CharaChorder and load its chord map, then start typing. Cadenza compares how often you fire each chord versus typing it out by hand."
            />
          </CardContent>
        </Card>
      ) : (
        <div className="space-y-8">
          {showPractice && (
            <section>
              <div className="mb-3 flex items-center gap-2">
                <Gauge className="size-4 text-gold" />
                <h2 className="text-sm font-medium text-foreground">Needs practice</h2>
                <Badge variant="outline" className="tnum text-muted-foreground">{practice.length}</Badge>
              </div>
              {practice.length ? (
                <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
                  {practice.map((p) => (
                    <ProfCard key={p.phrase} p={p} />
                  ))}
                </div>
              ) : (
                <EmptyState compact icon={CheckCircle2} title="All chords mastered" hint="Nothing needs practice right now." />
              )}
            </section>
          )}

          {showMastered && (
            <section>
              <div className="mb-3 flex items-center gap-2">
                <CheckCircle2 className="size-4 text-success" />
                <h2 className="text-sm font-medium text-foreground">Mastered</h2>
                <Badge variant="outline" className="tnum text-muted-foreground">{mastered.length}</Badge>
              </div>
              {mastered.length ? (
                <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
                  {mastered.map((p) => (
                    <ProfCard key={p.phrase} p={p} />
                  ))}
                </div>
              ) : (
                <EmptyState compact icon={Target} title="None mastered yet" hint="Keep firing your chords to master them." />
              )}
            </section>
          )}
        </div>
      )}
    </div>
  );
}
