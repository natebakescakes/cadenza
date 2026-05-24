import { motion } from "framer-motion";
import { cn } from "@/lib/utils";
import { clamp } from "@/lib/format";

type Tone = "accent" | "success" | "warning" | "danger" | "info";

export interface ProgressBarProps {
  /** 0..1 */
  value: number;
  tone?: Tone;
  className?: string;
  size?: "sm" | "md";
  "aria-label"?: string;
}

const fills: Record<Tone, string> = {
  accent: "bg-gold",
  success: "bg-success",
  warning: "bg-gold-bright",
  danger: "bg-danger",
  info: "bg-info",
};

/** Tone-aware, spring-animated progress bar (0..1) on the Cadenza palette. */
export function ProgressBar({
  value,
  tone = "accent",
  className,
  size = "md",
  "aria-label": ariaLabel,
}: ProgressBarProps) {
  const pct = clamp(value) * 100;
  return (
    <div
      role="progressbar"
      aria-valuenow={Math.round(pct)}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-label={ariaLabel}
      className={cn(
        "w-full overflow-hidden rounded-full bg-secondary",
        size === "sm" ? "h-1.5" : "h-2",
        className,
      )}
    >
      <motion.div
        className={cn("h-full rounded-full", fills[tone])}
        initial={{ width: 0 }}
        animate={{ width: `${pct}%` }}
        transition={{ type: "spring", stiffness: 120, damping: 22 }}
      />
    </div>
  );
}
