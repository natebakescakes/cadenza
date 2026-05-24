import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Activity, Gauge, Hand, Keyboard, Sparkles, TrendingUp, Zap } from "lucide-react";
import {
  Bar, BarChart, CartesianGrid, Cell, ResponsiveContainer,
  Tooltip as RTooltip, XAxis, YAxis,
} from "recharts";
import { PageHeader } from "@/components/PageHeader";
import { StatCard } from "@/components/StatCard";
import { EmptyState } from "@/components/EmptyState";
import { WpmChart } from "@/components/WpmChart";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useWpmSummary, useWpmTrend, type WpmRange } from "@/hooks/useWpm";
import { useLiveSession, type LiveBlock } from "@/hooks/useLiveSession";
import { formatWpm } from "@/lib/format";
import { cn } from "@/lib/utils";

const BLOCK_MS = 5 * 60 * 1000;
const RANGES: { value: WpmRange; label: string }[] = [
  { value: "live",  label: "Live" },
  { value: "day",   label: "Day" },
  { value: "week",  label: "Week" },
  { value: "month", label: "Month" },
];

// ---------------------------------------------------------------------------
// Historical compare tooltip
// ---------------------------------------------------------------------------
interface CompareEntry {
  dataKey: string;
  value: number;
  fill?: string;
  payload: { name: string };
}
interface CompareTooltipProps {
  active?: boolean;
  payload?: CompareEntry[];
}

const CompareTooltip = ({ active, payload }: CompareTooltipProps) => {
  if (!active || !payload?.length) return null;
  return (
    <div className="rounded-lg border border-border bg-popover/95 px-3 py-2 text-xs shadow-lg backdrop-blur">
      {payload.map((p) => (
        <p key={p.dataKey} className="tnum flex items-center gap-2">
          <span className="size-2 rounded-full" style={{ background: p.fill }} />
          <span className="text-muted-foreground">{p.payload.name}</span>
          <span className="ml-auto font-medium text-foreground">{Math.round(p.value)} wpm</span>
        </p>
      ))}
    </div>
  );
};

// ---------------------------------------------------------------------------
// Live tab — word chips + block cards
// ---------------------------------------------------------------------------
function blockLabel(blockStart: number): string {
  const start = new Date(blockStart);
  const end   = new Date(blockStart + BLOCK_MS);
  const fmt = (d: Date) =>
    d.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  return `${fmt(start)} – ${fmt(end)}`;
}

function WordChip({ text, source }: { text: string; source: "manual" | "chorded" }) {
  return (
    <motion.span
      layout
      initial={{ opacity: 0, scale: 0.88 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.22, ease: [0.16, 1, 0.3, 1] }}
      className={cn(
        "inline-flex items-center rounded-md border px-2 py-0.5 font-mono text-xs font-medium",
        source === "chorded"
          ? "border-info/30 bg-info/10 text-info"
          : "border-border bg-secondary/60 text-foreground/80",
      )}
    >
      {text}
      {source === "chorded" && <Zap className="ml-1 size-2.5 shrink-0 opacity-70" />}
    </motion.span>
  );
}

function BlockCard({ block, isLatest }: { block: LiveBlock; isLatest: boolean }) {
  const allManual  = [...block.manualWords,    ...block.liveEntries.filter(e => e.source === "manual").map(e => e.text)];
  const allChorded = [...block.chorded_words,  ...block.liveEntries.filter(e => e.source === "chorded").map(e => e.text)];
  const total = allManual.length + allChorded.length;

  return (
    <motion.div layout initial={{ opacity: 0, y: 14 }} animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.35, ease: [0.16, 1, 0.3, 1] }}>
      <Card className={cn("gap-0", isLatest && "ring-1 ring-gold/30")}>
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-2">
              {isLatest && <span className="size-2 rounded-full bg-success animate-pulse-soft" />}
              <CardTitle className="text-sm font-medium text-foreground">
                {blockLabel(block.blockStart)}
              </CardTitle>
            </div>
            <div className="flex items-center gap-2">
              {allManual.length > 0 && (
                <Badge variant="outline" className="tnum gap-1 text-xs text-muted-foreground">
                  <Keyboard className="size-3" /> {allManual.length}
                </Badge>
              )}
              {allChorded.length > 0 && (
                <Badge variant="outline" className="tnum gap-1 text-xs text-info">
                  <Zap className="size-3" /> {allChorded.length}
                </Badge>
              )}
              {block.wpm > 0 && (
                <Badge className="tnum bg-gold/15 text-gold border-gold/25 text-xs font-semibold">
                  {formatWpm(block.wpm)} wpm
                </Badge>
              )}
            </div>
          </div>
        </CardHeader>
        <CardContent>
          {total === 0 ? (
            <p className="text-xs italic text-muted-foreground/60">No words yet in this window.</p>
          ) : (
            <div className="flex flex-wrap gap-1.5">
              <AnimatePresence initial={false}>
                {allManual.map((w, i) => (
                  <WordChip key={`m-${i}`} text={w} source="manual" />
                ))}
                {allChorded.map((w, i) => (
                  <WordChip key={`c-${i}`} text={w} source="chorded" />
                ))}
              </AnimatePresence>
            </div>
          )}
        </CardContent>
      </Card>
    </motion.div>
  );
}

// ---------------------------------------------------------------------------
// Main page
// ---------------------------------------------------------------------------
export default function Wpm() {
  const [range, setRange] = useState<WpmRange>("live");
  const isLive = range === "live";

  const { data: summary }       = useWpmSummary();
  const { data: trend }         = useWpmTrend(range);
  const { currentWpm, blocks }  = useLiveSession();

  const compareData = [
    { name: "Chorded", value: summary.chorded, fill: "var(--color-info)" },
    { name: "Manual",  value: summary.manual,  fill: "var(--color-success)" },
    { name: "Overall", value: summary.overall, fill: "var(--color-gold)" },
  ];
  const hasCompare  = summary.chorded > 0 || summary.manual > 0 || summary.overall > 0;
  const hasLiveData = blocks.some(
    b => b.manualWords.length > 0 || b.chorded_words.length > 0 || b.liveEntries.length > 0
  );

  return (
    <div>
      <PageHeader
        title="Analytics"
        subtitle="Track your pace across chorded and manual typing."
        actions={
          <Tabs value={range} onValueChange={(v) => setRange(v as WpmRange)}>
            <TabsList>
              {RANGES.map((r) => (
                <TabsTrigger key={r.value} value={r.value}>
                  {r.value === "live" && isLive && (
                    <span className="mr-1.5 inline-block size-1.5 rounded-full bg-success animate-pulse-soft" />
                  )}
                  {r.label}
                </TabsTrigger>
              ))}
            </TabsList>
          </Tabs>
        }
      />

      {/* Stat row — always visible, staggered entrance */}
      <motion.div
        className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-5"
        initial="hidden"
        animate="show"
        variants={{
          hidden: {},
          show: { transition: { staggerChildren: 0.05 } },
        }}
      >
        {[
          { label: "60 sec", value: formatWpm(currentWpm ?? summary.rolling), icon: Activity, accent: true },
          { label: "Session", value: formatWpm(summary.session), icon: Gauge },
          { label: "Overall", value: formatWpm(summary.overall), icon: TrendingUp },
          { label: "Chorded", value: formatWpm(summary.chorded), icon: Sparkles },
          { label: "Manual", value: formatWpm(summary.manual), icon: Hand },
        ].map((s) => (
          <motion.div
            key={s.label}
            variants={{
              hidden: { opacity: 0, y: 12 },
              show: { opacity: 1, y: 0, transition: { duration: 0.4, ease: [0.16, 1, 0.3, 1] } },
            }}
          >
            <StatCard label={s.label} value={s.value} unit="wpm" icon={s.icon} accent={s.accent} />
          </motion.div>
        ))}
      </motion.div>

      {/* Tab content */}
      <AnimatePresence mode="wait">
        {isLive ? (
          <motion.div key="live" initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }} transition={{ duration: 0.28, ease: [0.16, 1, 0.3, 1] }}
            className="mt-4">
            {/* 5-min blocks — history + live merged */}
            {!hasLiveData ? (
              <Card>
                <CardContent>
                  <EmptyState
                    icon={Activity}
                    title="Waiting for keystrokes"
                    hint="Start typing — words and chords appear here grouped into 5-minute windows."
                  />
                </CardContent>
              </Card>
            ) : (
              <div className="space-y-3">
                <AnimatePresence initial={false}>
                  {blocks.map((block, i) => (
                    <BlockCard key={block.blockStart} block={block} isLatest={i === 0} />
                  ))}
                </AnimatePresence>
              </div>
            )}
          </motion.div>
        ) : (
          <motion.div key={range} initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }} transition={{ duration: 0.28, ease: [0.16, 1, 0.3, 1] }}
            className="mt-4 grid grid-cols-1 gap-4 lg:grid-cols-3">
            <div className="lg:col-span-2">
              <Card className="h-full">
                <CardHeader>
                  <CardTitle>Pace over time</CardTitle>
                  <p className="text-xs text-muted-foreground">
                    {RANGES.find((r) => r.value === range)?.label} view
                  </p>
                </CardHeader>
                <CardContent>
                  {trend.length > 1 ? (
                    <WpmChart samples={trend} height={300} />
                  ) : (
                    <EmptyState icon={TrendingUp} title="Not enough data yet"
                      hint="Keep typing — your trend will appear once a few samples are recorded." />
                  )}
                </CardContent>
              </Card>
            </div>
            <div>
              <Card className="h-full">
                <CardHeader>
                  <CardTitle>Chorded vs manual</CardTitle>
                  <p className="text-xs text-muted-foreground">Where your speed comes from</p>
                </CardHeader>
                <CardContent>
                  {hasCompare ? (
                    <div className="h-[300px] w-full">
                      <ResponsiveContainer width="100%" height="100%">
                        <BarChart data={compareData} margin={{ top: 8, right: 8, left: -20, bottom: 0 }}>
                          <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} opacity={0.5} />
                          <XAxis dataKey="name" stroke="var(--muted-foreground)" fontSize={11} tickLine={false} axisLine={false} />
                          <YAxis stroke="var(--muted-foreground)" fontSize={11} tickLine={false} axisLine={false} />
                          <RTooltip content={<CompareTooltip />} cursor={{ fill: "var(--secondary)" }} />
                          <Bar dataKey="value" radius={[6, 6, 0, 0]} maxBarSize={56}>
                            {compareData.map((d) => (
                              <Cell key={d.name} fill={d.fill} />
                            ))}
                          </Bar>
                        </BarChart>
                      </ResponsiveContainer>
                    </div>
                  ) : (
                    <EmptyState icon={Sparkles} title="No split yet"
                      hint="Fire some chords and type some words to compare." />
                  )}
                </CardContent>
              </Card>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
