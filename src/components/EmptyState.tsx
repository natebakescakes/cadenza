import type { ComponentType, ReactNode } from "react";
import { motion } from "framer-motion";
import type { LucideProps } from "lucide-react";
import { cn } from "@/lib/utils";

export interface EmptyStateProps {
  icon: ComponentType<LucideProps>;
  title: string;
  hint?: string;
  action?: ReactNode;
  className?: string;
  compact?: boolean;
}

/** Calm, encouraging empty state — used heavily while the backend is empty. */
export function EmptyState({
  icon: Icon,
  title,
  hint,
  action,
  className,
  compact,
}: EmptyStateProps) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4, ease: [0.16, 1, 0.3, 1] }}
      className={cn(
        "flex flex-col items-center justify-center text-center",
        compact ? "gap-2 py-8" : "gap-3.5 py-14",
        className,
      )}
    >
      <div
        className={cn(
          "relative grid place-items-center rounded-2xl border border-border bg-secondary/50",
          "before:absolute before:inset-0 before:rounded-2xl before:bg-[radial-gradient(circle_at_center,color-mix(in_oklch,var(--color-gold)_14%,transparent),transparent_70%)]",
          compact ? "size-10" : "size-14",
        )}
      >
        <Icon
          className={cn(
            "relative text-muted-foreground/70",
            compact ? "size-5" : "size-6",
          )}
          strokeWidth={1.5}
        />
      </div>
      <div className="space-y-1">
        <p
          className={cn(
            "font-medium text-foreground",
            compact ? "text-sm" : "text-base",
          )}
        >
          {title}
        </p>
        {hint && (
          <p className="mx-auto max-w-xs text-xs leading-relaxed text-muted-foreground">
            {hint}
          </p>
        )}
      </div>
      {action && <div className="mt-1">{action}</div>}
    </motion.div>
  );
}
