import type { ReactNode } from "react";
import { motion } from "framer-motion";
import { cn } from "@/lib/utils";

export interface PageHeaderProps {
  title: string;
  subtitle?: string;
  actions?: ReactNode;
  /** Override the default bottom margin (e.g. tighter on dense layouts). */
  className?: string;
}

/** Editorial page header with serif display title. */
export function PageHeader({ title, subtitle, actions, className }: PageHeaderProps) {
  return (
    <div className={cn("mb-8 flex items-end justify-between gap-6", className)}>
      <div className="min-w-0">
        <motion.h1
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5, ease: [0.16, 1, 0.3, 1] }}
          className="font-display text-[2rem] leading-none font-semibold tracking-[-0.02em] text-foreground"
        >
          {title}
        </motion.h1>
        {subtitle && (
          <motion.p
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ duration: 0.5, delay: 0.06 }}
            className="mt-2 max-w-prose text-sm text-balance text-muted-foreground"
          >
            {subtitle}
          </motion.p>
        )}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </div>
  );
}
