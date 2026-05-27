import { useEffect } from "react";
import { motion } from "framer-motion";
import { AlertTriangle, CheckCircle2, Loader2 } from "lucide-react";
import { acquireVisibility, releaseVisibility } from "@/lib/overlayPanel";
import type { SyncSurfacePayload } from "@/lib/types";

// How long the terminal (done/error) states linger before auto-hiding.
const DONE_AUTO_HIDE_MS = 1500;
const ERROR_AUTO_HIDE_MS = 4000;

const ENTER_EASE: [number, number, number, number] = [0.16, 1, 0.3, 1];

interface SyncSurfaceProps {
  payload: SyncSurfacePayload;
  /** Remove this surface from the stack (drives the AnimatePresence exit upstream). */
  onDone: () => void;
}

/**
 * Sync surface — chord-library refresh progress. Driven entirely by props from
 * the overlay root (it listens for nothing itself). Non-interactive, so it only
 * acquires the panel arbiter's VISIBILITY (never interactivity). Terminal states
 * (done/error) auto-hide after a short linger. Matches the overlay's visual
 * language: rounded card, bg-popover/95, border, backdrop-blur, quiet.
 */
export function SyncSurface({ payload, onDone }: SyncSurfaceProps) {
  // Hold the shared panel visible while mounted so coaching's auto-hide can't
  // tear the NSPanel down underneath us.
  useEffect(() => {
    acquireVisibility();
    return () => releaseVisibility();
  }, []);

  // Auto-hide on terminal states. Re-armed if the state transitions.
  useEffect(() => {
    if (payload.state === "syncing") return;
    const delay = payload.state === "done" ? DONE_AUTO_HIDE_MS : ERROR_AUTO_HIDE_MS;
    const t = setTimeout(onDone, delay);
    return () => clearTimeout(t);
  }, [payload.state, onDone]);

  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.9 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0, scale: 0.95 }}
      transition={{ duration: 0.22, ease: ENTER_EASE }}
      className="inline-flex items-center gap-2 rounded-xl border border-border bg-popover/95 px-3 py-2 shadow-lg backdrop-blur-sm"
    >
      {payload.state === "syncing" && (
        <>
          <Loader2 className="size-3.5 shrink-0 animate-spin text-muted-foreground/70" />
          <span className="text-[11px] text-foreground/80">Syncing chords…</span>
        </>
      )}
      {payload.state === "done" && (
        <>
          <CheckCircle2 className="size-3.5 shrink-0 text-emerald-400/80" />
          <span className="text-[11px] text-foreground/80">
            {typeof payload.count === "number"
              ? `${payload.count} chords synced`
              : "Chords synced"}
          </span>
        </>
      )}
      {payload.state === "error" && (
        <>
          <AlertTriangle className="size-3.5 shrink-0 text-amber-400/90" />
          <span className="text-[11px] text-foreground/80">
            {payload.message ?? "Chord sync failed"}
          </span>
        </>
      )}
    </motion.div>
  );
}
