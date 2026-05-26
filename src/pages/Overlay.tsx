import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { AlertTriangle, ArrowLeftRight, CheckCircle2 } from "lucide-react";
import { ComboKeys } from "@/components/ComboKeys";
import {
  getSettings,
  hideOverlay,
  onCoachingDismiss,
  onCoachingHint,
  onCoachingPosition,
} from "@/lib/api";
import type { CoachingCombo, CoachingHint } from "@/lib/types";

// Defaults mirror src/hooks/useSettings.ts; getSettings overrides on mount.
const DEFAULT_SHOW_MS = 1500;
const DEFAULT_FADE_MS = 300;
// If no coaching_position arrives within this grace window, proceed anyway —
// the NSPanel may already be positioned in Rust. Used only to ignore stale ids.
const POSITION_GRACE_MS = 250;

/**
 * Determines whether a hint should render in "options" (rich) mode.
 * Triggers on:
 *   - source === "suggested" (no chord yet, deciding surface)
 *   - any alternatives exist (device chord + suggested alts, or conflict + alts)
 */
function isOptionsMode(hint: CoachingHint): boolean {
  if (hint.source === "suggested") return true;
  if (hint.alt_count > 0) return true;
  return false;
}

/**
 * Reorder alternatives free → swap → hard-taken so the user sees the cleanest
 * recommendation first, then actionable swaps, then dead entries.
 * Primary (index 0) stays put — we only reorder alternatives.
 *   - free:  no conflicts.
 *   - swap:  occupied but a reassignment is suggested (swap_target set).
 *   - taken: occupied with no swap suggestion.
 */
function sortedAlternatives(combos: CoachingCombo[]): CoachingCombo[] {
  if (!combos || combos.length <= 1) return [];
  const [primary, ...alts] = combos;
  void primary; // primary handled separately
  const free = alts.filter((c) => c.conflicts.length === 0);
  const swap = alts.filter((c) => c.conflicts.length > 0 && c.swap_target);
  const taken = alts.filter((c) => c.conflicts.length > 0 && !c.swap_target);
  return [...free, ...swap, ...taken];
}

// Typed cubic-bezier for framer-motion (number[] is rejected by Variants type).
const ALT_EASE: [number, number, number, number] = [0.16, 1, 0.3, 1];

// ── Sub-components ────────────────────────────────────────────────────────────

/** Compact amber chip listing the words that conflict with a combo. */
function ConflictChip({ conflicts }: { conflicts: string[] }) {
  if (conflicts.length === 0) return null;
  const label =
    conflicts.length === 1
      ? `used by "${conflicts[0]}"`
      : `used by "${conflicts[0]}" +${conflicts.length - 1}`;
  return (
    <span className="inline-flex items-center gap-1 rounded-md border border-amber-500/25 bg-amber-500/10 px-1.5 py-px font-mono text-[9px] leading-none text-amber-400/90">
      <AlertTriangle className="size-2.5 shrink-0" />
      {label}
    </span>
  );
}

/** A small "free" indicator when a combo has no conflicts. */
function FreeChip() {
  return (
    <span className="inline-flex items-center gap-1 rounded-md border border-emerald-500/20 bg-emerald-500/8 px-1.5 py-px font-mono text-[9px] leading-none text-emerald-400/70">
      <CheckCircle2 className="size-2.5 shrink-0" />
      free
    </span>
  );
}

/**
 * Occupied-but-reassignable indicator. The combo is taken, but the current word
 * out-uses the holder enough that we suggest swapping it. Recommend-only — the
 * user remaps manually. `reason` (if present) is shown as a tooltip.
 */
function SwapChip({ target, reason }: { target: string; reason?: string | null }) {
  return (
    <span
      title={reason ?? undefined}
      className="inline-flex items-center gap-1 rounded-md border border-sky-500/30 bg-sky-500/10 px-1.5 py-px font-mono text-[9px] leading-none text-sky-300/90"
    >
      <ArrowLeftRight className="size-2.5 shrink-0" />
      swap from "{target}"
    </span>
  );
}

interface ViewProps {
  hint: CoachingHint;
  fadeMs: number;
  onMouseEnter: () => void;
  onMouseLeave: () => void;
}

// ── Reminder mode (quiet, minimal — unchanged feel) ───────────────────────────

function ReminderView({ hint, fadeMs, onMouseEnter, onMouseLeave }: ViewProps) {
  // All existing device mappings for this word (primary first). Fall back to
  // the flat primary_combo if combos is somehow empty.
  const combos =
    hint.combos && hint.combos.length > 0
      ? hint.combos.map((c) => c.combo)
      : [hint.primary_combo];

  // Single mapping: keep the quiet, minimal inline pill — now includes the phrase.
  if (combos.length === 1) {
    return (
      <motion.div
        key={hint.id}
        initial={{ opacity: 0, scale: 0.9 }}
        animate={{ opacity: 1, scale: 1 }}
        exit={{ opacity: 0, scale: 0.95 }}
        transition={{ duration: fadeMs / 1000, ease: ALT_EASE }}
        className="inline-flex items-center gap-2 rounded-xl border border-border bg-popover/95 px-3 py-2 shadow-lg backdrop-blur-sm"
        onMouseEnter={onMouseEnter}
        onMouseLeave={onMouseLeave}
      >
        <span className="font-mono text-[10px] text-muted-foreground/60">{hint.phrase}</span>
        <span className="h-3 w-px shrink-0 bg-border/60" />
        <ComboKeys combo={combos[0]} />
      </motion.div>
    );
  }

  // Multiple mappings: expand to show every combination, stacked. Still quiet
  // (no conflicts here — these are the user's own chords), just complete.
  return (
    <motion.div
      key={hint.id}
      initial={{ opacity: 0, scale: 0.96 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0, scale: 0.97 }}
      transition={{ duration: fadeMs / 1000, ease: ALT_EASE }}
      className="inline-flex flex-col gap-1.5 rounded-xl border border-border bg-popover/95 px-3 py-2 shadow-lg backdrop-blur-sm"
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
    >
      <span className="text-[9px] font-medium uppercase tracking-widest text-muted-foreground/60">
        {combos.length} chords
      </span>
      {combos.map((c, i) => (
        <motion.div
          key={`${c}-${i}`}
          custom={i}
          variants={ALT_VARIANTS}
          initial="hidden"
          animate="visible"
        >
          <ComboKeys combo={c} />
        </motion.div>
      ))}
    </motion.div>
  );
}

// ── Options mode (rich, dense, decision surface) ──────────────────────────────

const ALT_VARIANTS = {
  hidden: { opacity: 0, x: -4 },
  visible: (i: number) => ({
    opacity: 1,
    x: 0,
    transition: { duration: 0.18, delay: 0.08 + i * 0.055, ease: ALT_EASE },
  }),
};

function OptionsView({ hint, fadeMs, onMouseEnter, onMouseLeave }: ViewProps) {
  const primary = hint.combos?.[0];
  const alternatives = sortedAlternatives(hint.combos ?? []);
  const hasAlts = alternatives.length > 0;

  return (
    <motion.div
      key={hint.id}
      initial={{ opacity: 0, y: -6, scale: 0.96 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, scale: 0.97 }}
      transition={{ duration: fadeMs / 1000, ease: ALT_EASE }}
      className="w-[300px] rounded-xl border border-border bg-popover/97 shadow-xl backdrop-blur-md"
      style={{ transformOrigin: "top left" }}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
    >
      {/* Header: word + intent label */}
      <div className="flex items-baseline justify-between gap-3 border-b border-border/60 px-3 pt-2.5 pb-2">
        <span className="font-mono text-[11px] font-semibold tracking-wide text-foreground/90">
          {hint.phrase}
        </span>
        <span className="shrink-0 rounded-full border border-gold/25 bg-gold/12 px-1.5 py-px text-[9px] font-medium uppercase tracking-widest text-gold/80">
          {hint.source === "suggested"
            ? primary?.swap_target
              ? "swap available"
              : "no chord yet"
            : hint.combos?.[0]?.conflicts.length
              ? "conflict"
              : "alternatives"}
        </span>
      </div>

      {/* Primary combo */}
      <div className="px-3 pt-2.5 pb-2">
        <p className="mb-1.5 text-[9px] font-medium uppercase tracking-widest text-muted-foreground/60">
          {hint.source === "device" ? "your chord" : "suggested"}
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <ComboKeys combo={hint.primary_combo} />
          {primary && primary.swap_target ? (
            <SwapChip target={primary.swap_target} reason={primary.swap_reason} />
          ) : primary && primary.conflicts.length > 0 ? (
            <ConflictChip conflicts={primary.conflicts} />
          ) : hint.source !== "device" ? (
            <FreeChip />
          ) : null}
        </div>
      </div>

      {/* Alternatives — scrollable with max height + fade hint */}
      {hasAlts && (
        <>
          <div className="mx-3 border-t border-border/40" />
          <div className="relative">
            <div className="max-h-[220px] overflow-y-auto overscroll-contain px-3 pt-2 pb-2.5 space-y-1.5">
              <p className="mb-1 text-[9px] font-medium uppercase tracking-widest text-muted-foreground/50">
                {hint.source === "device" ? "try instead" : "alternatives"}
              </p>
              {alternatives.map((alt, i) => (
                <motion.div
                  key={alt.combo}
                  custom={i}
                  variants={ALT_VARIANTS}
                  initial="hidden"
                  animate="visible"
                  className="flex flex-wrap items-center gap-2 pl-2 border-l border-border/40"
                >
                  <span className="opacity-75">
                    <ComboKeys combo={alt.combo} />
                  </span>
                  {alt.swap_target ? (
                    <SwapChip target={alt.swap_target} reason={alt.swap_reason} />
                  ) : alt.conflicts.length > 0 ? (
                    <ConflictChip conflicts={alt.conflicts} />
                  ) : (
                    <FreeChip />
                  )}
                </motion.div>
              ))}
            </div>
            {/* Gradient fade at bottom to hint at scrollability */}
            <div className="pointer-events-none absolute bottom-0 left-0 right-0 h-5 bg-gradient-to-t from-popover/97 to-transparent" />
          </div>
        </>
      )}
    </motion.div>
  );
}

// ── Root overlay ──────────────────────────────────────────────────────────────

/**
 * Bare coaching overlay. Renders the chord mapping for a just-typed word.
 *
 * Runs in its own React root (overlay-main.tsx) with NO providers — no DbGate,
 * AppShell, HashRouter, Toaster, or theme context beyond the `.dark` class.
 * The NSPanel is positioned in Rust on `coaching_position`; this webview only
 * renders content at its own origin and drives the show/fade/dismiss lifecycle.
 */
export default function Overlay() {
  const [hint, setHint] = useState<CoachingHint | null>(null);
  const [visible, setVisible] = useState(false);

  // Live show/fade timings + persist flag; refreshed from settings on mount.
  const showMsRef = useRef(DEFAULT_SHOW_MS);
  const fadeMsRef = useRef(DEFAULT_FADE_MS);
  const persistRef = useRef(false);
  // The currently-displayed hint id, so stale position events are ignored.
  const currentIdRef = useRef<number | null>(null);
  // Pending hold timer; cleared when a new hint or a keystroke supersedes it.
  const hideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // True while the pointer is over the overlay — pauses the auto-hide timer.
  const hoveredRef = useRef(false);

  // Lifted to component scope so mouse handlers can share them with useEffect.
  const clearHideTimer = () => {
    if (hideTimerRef.current !== null) {
      clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
  };

  const scheduleHide = () => {
    clearHideTimer();
    hideTimerRef.current = setTimeout(() => setVisible(false), showMsRef.current);
  };

  const handleMouseEnter = () => {
    hoveredRef.current = true;
    clearHideTimer(); // pause auto-dismiss while pointer is over the overlay
  };

  const handleMouseLeave = () => {
    hoveredRef.current = false;
    if (!persistRef.current) {
      scheduleHide(); // restart full timer after pointer leaves
    }
  };

  useEffect(() => {
    let mounted = true;

    getSettings()
      .then((s) => {
        if (!mounted) return;
        if (typeof s.coaching_show_ms === "number") showMsRef.current = s.coaching_show_ms;
        if (typeof s.coaching_fade_ms === "number") fadeMsRef.current = s.coaching_fade_ms;
        if (typeof s.coaching_persist === "boolean") persistRef.current = s.coaching_persist;
      })
      .catch(() => {
        // Fall back to defaults — the overlay still works.
      });

    const dismiss = () => {
      // The backend decides WHEN to send `coaching_dismiss` per mode (every
      // keystroke in normal mode; only when a new word begins in persist mode),
      // so we just honor it here regardless of mode.
      clearHideTimer();
      setVisible(false);
    };

    const unlistenHint = onCoachingHint((h) => {
      // Each hint carries a LIVE settings snapshot (the overlay window is
      // long-lived and can't see Settings edits made after it mounted), so
      // refresh from the payload before driving the lifecycle.
      if (typeof h.persist === "boolean") persistRef.current = h.persist;
      if (typeof h.show_ms === "number") showMsRef.current = h.show_ms;
      if (typeof h.fade_ms === "number") fadeMsRef.current = h.fade_ms;

      currentIdRef.current = h.id;
      setHint(h);
      setVisible(true);
      clearHideTimer();

      // Don't start the hide timer if the pointer is already over the overlay
      // (user hovered before new hint arrived).
      if (!persistRef.current && !hoveredRef.current) {
        // Hold fully visible for show_ms, then fade out over fade_ms.
        hideTimerRef.current = setTimeout(() => {
          setVisible(false);
        }, showMsRef.current);
      }
      // In persist mode: no timer started; stays until next hint replaces it.
    });

    // Positioning is done in Rust; we only use the event to ignore stale ids.
    const unlistenPosition = onCoachingPosition((p) => {
      if (currentIdRef.current !== null && p.id < currentIdRef.current) {
        // Stale position for an older hint — ignore.
        return;
      }
    });
    void POSITION_GRACE_MS;

    // Instant dismiss: the backend emits an EMPTY `coaching_dismiss` signal
    // (privacy: no typed char is ever shipped here). Normal mode emits it on the
    // next keystroke + from its clear-timer floor; persist mode emits it only
    // when a new word begins. Either way, hide.
    const unlistenDismiss = onCoachingDismiss(() => {
      dismiss();
    });

    return () => {
      mounted = false;
      clearHideTimer();
      void unlistenHint.then((fn) => fn());
      void unlistenPosition.then((fn) => fn());
      void unlistenDismiss.then((fn) => fn());
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Derive mode from current hint (memoised implicitly — recomputed only on render).
  const optionsMode = hint ? isOptionsMode(hint) : false;

  return (
    // Anchor BOTTOM-left: the panel is positioned ABOVE the caret (Rust sets the
    // panel so its bottom edge sits just above the caret), so the content must
    // hug the bottom of the panel. Fill the panel viewport (h-screen) and push
    // content to the bottom-left; the transparent area extends upward, invisible.
    <div className="flex h-screen w-screen items-end justify-start bg-transparent p-1">
      <AnimatePresence
        onExitComplete={() => {
          // Fade-out finished: hide the NSPanel itself so a transparent empty
          // panel doesn't linger. Applies in both modes — the backend decides
          // WHEN to dismiss (persist clears on a new word), and once content has
          // faded the panel should be hidden either way.
          void hideOverlay().catch(() => {});
        }}
      >
        {visible && hint && (
          optionsMode ? (
            <OptionsView
              key={hint.id}
              hint={hint}
              fadeMs={fadeMsRef.current}
              onMouseEnter={handleMouseEnter}
              onMouseLeave={handleMouseLeave}
            />
          ) : (
            <ReminderView
              key={hint.id}
              hint={hint}
              fadeMs={fadeMsRef.current}
              onMouseEnter={handleMouseEnter}
              onMouseLeave={handleMouseLeave}
            />
          )
        )}
      </AnimatePresence>
    </div>
  );
}
