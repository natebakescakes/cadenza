import { motion } from "framer-motion";
import { Activity, Gauge, Hand, Sparkles, TrendingUp } from "lucide-react";
import { StatCard } from "@/components/StatCard";
import { useWpmSummary } from "@/hooks/useWpm";
import { useLiveSession } from "@/hooks/useLiveSession";
import { formatWpm } from "@/lib/format";

/**
 * The full WPM stat set — rolling/60s, session, overall, chorded, manual.
 * Shared between the Dashboard and the Analytics (Wpm) page so the numbers
 * stay in lockstep. The live 60 s value is rendered as the accented card.
 */
export function WpmStatRow() {
  const { data: summary } = useWpmSummary();
  const { currentWpm } = useLiveSession();

  const stats = [
    { label: "60 sec", value: formatWpm(currentWpm ?? summary.rolling), icon: Activity, accent: true },
    { label: "Session", value: formatWpm(summary.session), icon: Gauge },
    { label: "Overall", value: formatWpm(summary.overall), icon: TrendingUp },
    { label: "Chorded", value: formatWpm(summary.chorded), icon: Sparkles },
    { label: "Manual", value: formatWpm(summary.manual), icon: Hand },
  ];

  return (
    <motion.div
      className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-5"
      initial="hidden"
      animate="show"
      variants={{
        hidden: {},
        show: { transition: { staggerChildren: 0.05 } },
      }}
    >
      {stats.map((s) => (
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
  );
}
