import { useMemo } from "react";
import {
  Area,
  AreaChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip as RTooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { WpmSample } from "@/lib/types";

export interface WpmChartProps {
  samples: WpmSample[];
  height?: number;
  /** Render compact (dashboard) variant: fewer axes. */
  compact?: boolean;
}

interface Point {
  t: number;
  overall?: number;
  chorded?: number;
  manual?: number;
}

/** Merge multi-source samples into time-bucketed rows. */
function toPoints(samples: WpmSample[]): Point[] {
  const map = new Map<number, Point>();
  for (const s of samples) {
    const existing = map.get(s.t) ?? { t: s.t };
    if (s.source === "chorded") existing.chorded = s.wpm;
    else if (s.source === "manual") existing.manual = s.wpm;
    else existing.overall = s.wpm;
    map.set(s.t, existing);
  }
  return [...map.values()].sort((a, b) => a.t - b.t);
}

function fmtTime(t: number): string {
  const ms = t < 1e11 ? t * 1000 : t;
  return new Date(ms).toLocaleString("en-US", {
    month: "short",
    day: "numeric",
    hour: "numeric",
  });
}

interface TooltipEntry {
  dataKey: string;
  value: number;
  stroke?: string;
}
interface ChartTooltipProps {
  active?: boolean;
  payload?: TooltipEntry[];
  label?: number;
}

const ChartTooltip = ({ active, payload, label }: ChartTooltipProps) => {
  if (!active || !payload?.length) return null;
  return (
    <div className="rounded-lg border border-border bg-popover/95 px-3 py-2 text-xs shadow-lg backdrop-blur">
      <p className="mb-1 font-medium text-muted-foreground">
        {fmtTime(label ?? 0)}
      </p>
      {payload.map((p) => (
        <p key={p.dataKey} className="tnum flex items-center gap-2">
          <span
            className="size-2 rounded-full"
            style={{ background: p.stroke }}
          />
          <span className="capitalize text-muted-foreground">{p.dataKey}</span>
          <span className="ml-auto font-medium text-foreground">
            {Math.round(p.value)}
          </span>
        </p>
      ))}
    </div>
  );
};

export function WpmChart({ samples, height = 260, compact }: WpmChartProps) {
  const data = useMemo(() => toPoints(samples), [samples]);

  return (
    <div style={{ height }} className="w-full">
      <ResponsiveContainer width="100%" height="100%">
        <AreaChart data={data} margin={{ top: 8, right: 8, bottom: 0, left: -16 }}>
          <defs>
            <linearGradient id="g-overall" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="var(--color-gold)" stopOpacity={0.3} />
              <stop offset="100%" stopColor="var(--color-gold)" stopOpacity={0} />
            </linearGradient>
            <linearGradient id="g-chorded" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="var(--color-info)" stopOpacity={0.22} />
              <stop offset="100%" stopColor="var(--color-info)" stopOpacity={0} />
            </linearGradient>
            <linearGradient id="g-manual" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="var(--color-success)" stopOpacity={0.18} />
              <stop offset="100%" stopColor="var(--color-success)" stopOpacity={0} />
            </linearGradient>
          </defs>
          <CartesianGrid
            strokeDasharray="3 3"
            stroke="var(--border)"
            vertical={false}
            opacity={0.5}
          />
          {!compact && (
            <XAxis
              dataKey="t"
              tickFormatter={fmtTime}
              stroke="var(--muted-foreground)"
              fontSize={11}
              tickLine={false}
              axisLine={false}
              minTickGap={48}
            />
          )}
          <YAxis
            stroke="var(--muted-foreground)"
            fontSize={11}
            tickLine={false}
            axisLine={false}
            width={36}
          />
          <RTooltip content={<ChartTooltip />} cursor={{ stroke: "var(--border)" }} />
          <Area
            type="monotone"
            dataKey="manual"
            stroke="var(--color-success)"
            strokeWidth={1.5}
            fill="url(#g-manual)"
            connectNulls
          />
          <Area
            type="monotone"
            dataKey="chorded"
            stroke="var(--color-info)"
            strokeWidth={1.5}
            fill="url(#g-chorded)"
            connectNulls
          />
          <Area
            type="monotone"
            dataKey="overall"
            stroke="var(--color-gold)"
            strokeWidth={2}
            fill="url(#g-overall)"
            connectNulls
          />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  );
}
