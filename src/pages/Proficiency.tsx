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

function ProfCard({ p }: { p: Prof }) {
  const tone = p.mastered ? "success" : p.error_rate > 0.3 ? "danger" : "warning";
  return (
    <motion.div
      layout
      initial={{ opacity: 0, scale: 0.97 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.3, ease: [0.16, 1, 0.3, 1] }}
    >
      <Card className="gap-3 py-4 transition-colors hover:ring-foreground/20">
        <CardContent className="space-y-3">
          <div className="flex items-center justify-between gap-2">
            <span className="font-mono text-sm font-medium text-foreground">{p.phrase}</span>
            <Badge variant={p.mastered ? "default" : "outline"} className={p.mastered ? "bg-success/15 text-success" : "text-muted-foreground"}>
              {p.mastered ? "Mastered" : "Practice"}
            </Badge>
          </div>

          {p.error_count > 0 && (
            <div className="space-y-1.5">
              <div className="flex items-center justify-between text-xs">
                <span className="text-muted-foreground">Delete rate</span>
                <span className="tnum font-medium text-foreground">
                  {formatPercent(p.error_rate)} ({p.error_count}×)
                </span>
              </div>
              <ProgressBar value={p.error_rate} tone={tone} size="sm" aria-label={`${p.phrase} delete rate`} />
            </div>
          )}

          <div className="space-y-1.5">
            <div className="flex items-center justify-between text-xs">
              <span className="text-muted-foreground">Usage rate</span>
              <span className="tnum font-medium text-foreground">{formatPercent(p.usage_rate)}</span>
            </div>
            <ProgressBar value={p.usage_rate} tone={p.mastered ? "success" : "warning"} size="sm" aria-label={`${p.phrase} usage rate`} />
          </div>

          <div className="grid grid-cols-2 gap-2 pt-1">
            <div className="rounded-lg border border-border bg-secondary/40 px-2.5 py-2">
              <p className="text-[10px] tracking-wider text-muted-foreground/70 uppercase">Fire speed</p>
              <p className="tnum mt-0.5 text-sm font-semibold text-foreground">{formatMs(p.avg_fire_ms)}</p>
            </div>
            <div className="rounded-lg border border-border bg-secondary/40 px-2.5 py-2">
              <p className="text-[10px] tracking-wider text-muted-foreground/70 uppercase">Fired</p>
              <p className="tnum mt-0.5 text-sm font-semibold text-foreground">{p.fired_count}×</p>
            </div>
          </div>
        </CardContent>
      </Card>
    </motion.div>
  );
}

export default function Proficiency() {
  const { data } = useProficiency();
  const [filter, setFilter] = useState<Filter>("all");

  const { mastered, practice } = useMemo(() => {
    return {
      mastered: data.filter((p) => p.mastered),
      practice: data.filter((p) => !p.mastered),
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
                <TooltipContent side="bottom" className="max-w-[260px] text-left">
                  A chord is <span className="text-success">mastered</span> when you fire it
                  at least 3× and delete it less than 10% of the time.{" "}
                  <span className="text-gold">Practice</span> chords are ones you've used but
                  still frequently botch (high delete rate).
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

      {data.length === 0 ? (
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
