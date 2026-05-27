import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Activity, Sparkles, TrendingUp } from "lucide-react";
import {
  Bar, BarChart, CartesianGrid, Cell, ResponsiveContainer,
  Tooltip as RTooltip, XAxis, YAxis,
} from "recharts";
import { PageHeader } from "@/components/PageHeader";
import { EmptyState } from "@/components/EmptyState";
import { WpmChart } from "@/components/WpmChart";
import { WpmStatRow } from "@/components/WpmStatRow";
import { ActivityFeed } from "@/components/ActivityFeed";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useWpmSummary, useWpmTrend, type WpmRange } from "@/hooks/useWpm";
import { useLiveSessionContext } from "@/hooks/LiveSessionContext";

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
// Main page
// ---------------------------------------------------------------------------
export default function Wpm() {
  const [range, setRange] = useState<WpmRange>("live");
  const isLive = range === "live";

  const { data: summary }       = useWpmSummary();
  const { data: trend }         = useWpmTrend(range);
  const { blocks }              = useLiveSessionContext();

  const compareData = [
    { name: "Chorded", value: summary.chorded, fill: "var(--color-info)" },
    { name: "Manual",  value: summary.manual,  fill: "var(--color-success)" },
    { name: "Overall", value: summary.overall, fill: "var(--color-gold)" },
  ];
  const hasCompare  = summary.chorded > 0 || summary.manual > 0 || summary.overall > 0;

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

      {/* Full WPM stat row — shared with the Dashboard so numbers match. */}
      <WpmStatRow />

      {/* Tab content */}
      <AnimatePresence mode="wait">
        {isLive ? (
          <motion.div key="live" initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }} transition={{ duration: 0.28, ease: [0.16, 1, 0.3, 1] }}
            className="mt-4 space-y-3">
            <ActivityFeed
              blocks={blocks}
              emptyHint="Start typing — words and chords appear here grouped into 5-minute windows."
            />
          </motion.div>
        ) : (
          <motion.div key={range} initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }} transition={{ duration: 0.28, ease: [0.16, 1, 0.3, 1] }}
            className="mt-4 space-y-4">
            <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
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
            </div>

            {/* Full activity history — the complete block list lives here so the
                Dashboard can stay glanceable. */}
            <Card>
              <CardHeader className="pb-3">
                <CardTitle className="flex items-center gap-2">
                  <Activity className="size-4 text-gold" /> Activity history
                </CardTitle>
                <p className="text-xs text-muted-foreground">
                  Every 5-minute window from your session
                </p>
              </CardHeader>
              <CardContent className="space-y-3">
                <ActivityFeed
                  blocks={blocks}
                  emptyHint="Start typing — words and chords appear here grouped into 5-minute windows."
                />
              </CardContent>
            </Card>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
