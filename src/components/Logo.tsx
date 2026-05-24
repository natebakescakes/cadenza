import { cn } from "@/lib/utils";

export interface LogoProps {
  className?: string;
  /** Size of the mark in px. */
  size?: number;
}

/**
 * Cadenza mark — a fermata (musical "hold" arc) cradling three stacked
 * dots that read as a chord struck at once. The arc = mastery/pause,
 * the stacked notes = a chord fired in a single motion.
 */
export function LogoMark({ className, size = 28 }: LogoProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 32 32"
      fill="none"
      className={className}
      aria-hidden
    >
      {/* fermata arc */}
      <path
        d="M5 19C5 12.0964 9.92487 7 16 7C22.0751 7 27 12.0964 27 19"
        stroke="currentColor"
        strokeWidth="2.1"
        strokeLinecap="round"
      />
      {/* three stacked chord dots, ascending */}
      <circle cx="11" cy="22.5" r="2.1" fill="currentColor" />
      <circle cx="16" cy="20.5" r="2.1" fill="currentColor" />
      <circle cx="21" cy="22.5" r="2.1" fill="currentColor" />
      {/* center accent: the held note */}
      <circle cx="16" cy="13" r="1.6" fill="currentColor" opacity="0.55" />
    </svg>
  );
}

/** Full wordmark: mark + "Cadenza" in the display serif. */
export function Logo({ className }: { className?: string }) {
  return (
    <div className={cn("flex items-center gap-2.5 select-none", className)}>
      <LogoMark className="text-gold" size={26} />
      <span className="font-display text-[1.35rem] font-semibold tracking-tight text-foreground">
        Cadenza
      </span>
    </div>
  );
}
