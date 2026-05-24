import type { ComponentType } from "react";
import { motion } from "framer-motion";
import type { LucideProps } from "lucide-react";
import { Area, AreaChart, ResponsiveContainer } from "recharts";
import { cn } from "@/lib/utils";
import { Card } from "@/components/ui/card";

export interface StatCardProps {
  label: string;
  value: string;
  unit?: string;
  icon?: ComponentType<LucideProps>;
  delta?: string;
  deltaPositive?: boolean;
  /** Optional sparkline data. */
  spark?: number[];
  accent?: boolean;
  /** Tighter padding + smaller value type for dense, glanceable rows. */
  compact?: boolean;
  className?: string;
}

export function StatCard({
  label,
  value,
  unit,
  icon: Icon,
  delta,
  deltaPositive,
  spark,
  accent,
  compact,
  className,
}: StatCardProps) {
  const sparkData = spark?.map((v, i) => ({ i, v })) ?? [];
  const sparkColor = accent ? "var(--color-gold)" : "var(--muted-foreground)";
  const gradId = `spark-${label.replace(/\s+/g, "-").toLowerCase()}`;

  return (
    <Card
      className={cn(
        "gap-0 py-0 transition-[transform,box-shadow,--tw-ring-color] duration-200 ease-out hover:-translate-y-0.5 hover:ring-foreground/20",
        accent && "hover:ring-gold/30",
        className,
      )}
    >
      <div
        className={cn(
          "flex items-start justify-between gap-3 px-5 pt-5",
          compact && "px-4 pt-3",
        )}
      >
        <span className="text-xs font-medium tracking-wider text-muted-foreground/80 uppercase">
          {label}
        </span>
        {Icon && (
          <Icon
            className={cn("size-4", accent ? "text-gold" : "text-muted-foreground/60")}
            strokeWidth={1.75}
          />
        )}
      </div>

      <div
        className={cn("flex items-end gap-2 px-5 pt-2.5", compact && "px-4 pt-1.5")}
      >
        <motion.span
          key={value}
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.4, ease: [0.16, 1, 0.3, 1] }}
          className={cn(
            "font-display tnum leading-none font-semibold tracking-[-0.02em]",
            compact ? "text-[1.75rem]" : "text-4xl",
            accent ? "text-gold" : "text-foreground",
          )}
        >
          {value}
        </motion.span>
        {unit && (
          <span className="pb-1 text-sm font-medium text-muted-foreground">
            {unit}
          </span>
        )}
      </div>

      <div
        className={cn(
          "flex min-h-[20px] items-center gap-2 px-5 pt-2 pb-3",
          compact && !delta && "min-h-0 pt-0 pb-3",
          compact && delta && "px-4 pt-1 pb-3",
        )}
      >
        {delta && (
          <span
            className={cn(
              "tnum text-xs font-medium",
              deltaPositive ? "text-success" : "text-muted-foreground",
            )}
          >
            {delta}
          </span>
        )}
      </div>

      {spark && spark.length > 1 && (
        <div className="h-10 w-full opacity-90">
          <ResponsiveContainer width="100%" height="100%">
            <AreaChart
              data={sparkData}
              margin={{ top: 4, right: 0, bottom: 0, left: 0 }}
            >
              <defs>
                <linearGradient id={gradId} x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor={sparkColor} stopOpacity={0.35} />
                  <stop offset="100%" stopColor={sparkColor} stopOpacity={0} />
                </linearGradient>
              </defs>
              <Area
                type="monotone"
                dataKey="v"
                stroke={sparkColor}
                strokeWidth={1.5}
                fill={`url(#${gradId})`}
                isAnimationActive
                animationDuration={700}
              />
            </AreaChart>
          </ResponsiveContainer>
        </div>
      )}
    </Card>
  );
}
