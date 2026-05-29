import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { AlertTriangle, ArrowLeftRight, Check, CheckCircle2, Plus, X } from "lucide-react";
import { ComboKeys } from "@/components/ComboKeys";
import {
  addChordRecommendation,
  coachLog,
  dismissOverlay,
  getSettings,
  onCoachingDismiss,
  onCoachingHint,
  onCoachingPosition,
} from "@/lib/api";
import {
  acquireInteractive,
  acquireVisibility,
  releaseInteractive,
  releaseVisibility,
} from "@/lib/overlayPanel";
import type { CoachingCombo, CoachingHint } from "@/lib/types";
import { cn } from "@/lib/utils";

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
  /** Non-null → render the "enable Text Metrics in {app}" note atop the card. */
  metricsApp?: string | null;
  onMouseEnter: () => void;
  onMouseLeave: () => void;
  onDismiss: () => void;
}

/** Adds (phrase, combo) to the "chords to add" queue. Recommend-only — flips to
 *  a brief checkmark on success so the user knows it landed; the Practice page's
 *  list refreshes off the backend `recommendations_changed` event. */
function AddChordButton({ phrase, combo }: { phrase: string; combo: string }) {
  const [added, setAdded] = useState(false);
  return (
    <button
      type="button"
      aria-label={added ? "Added to your list" : "Add to your list"}
      title={added ? "Added" : "Add to your chords-to-add list"}
      onClick={() => {
        void addChordRecommendation(phrase, combo);
        setAdded(true);
      }}
      className={cn(
        "inline-flex size-4 shrink-0 items-center justify-center rounded-md transition-colors",
        added
          ? "text-emerald-400/90"
          : "text-muted-foreground/40 hover:bg-secondary/80 hover:text-foreground/80",
      )}
    >
      {added ? <Check className="size-3" /> : <Plus className="size-3" />}
    </button>
  );
}

/** Small, quiet close button (×) shared by both overlay views. */
function DismissButton({ onDismiss }: { onDismiss: () => void }) {
  return (
    <button
      type="button"
      aria-label="Dismiss"
      onClick={onDismiss}
      className="inline-flex size-4 shrink-0 items-center justify-center rounded-md text-muted-foreground/40 transition-colors hover:bg-secondary/80 hover:text-foreground/80"
    >
      <X className="size-3" />
    </button>
  );
}

// ── Text Metrics prompt (Chromium browsers with the flag off) ─────────────────

/** Top-of-card note shown when the focused app is a Chromium browser (Dia, Arc,
 *  …) with Text Metrics accessibility disabled — there's no caret geometry, so
 *  the overlay is centred and this explains how to fix it. Rendered as a header
 *  strip inside the existing overlay card (not a separate dialog). */
function MetricsNote({ app }: { app: string }) {
  return (
    <div className="border-b border-border/60 px-3 py-2 text-[10px] leading-snug text-muted-foreground/70">
      For inline suggestions, enable{" "}
      <span className="font-medium text-foreground/90">Text Metrics</span> in {app}{" "}
      Accessibility settings.
    </div>
  );
}

// ── Reminder mode (quiet, minimal — unchanged feel) ───────────────────────────

function ReminderView({ hint, fadeMs, metricsApp, onMouseEnter, onMouseLeave, onDismiss }: ViewProps) {
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
        className="inline-flex flex-col overflow-hidden rounded-xl border border-border bg-popover/95 shadow-lg backdrop-blur-sm"
        onMouseEnter={onMouseEnter}
        onMouseLeave={onMouseLeave}
      >
        {metricsApp && <MetricsNote app={metricsApp} />}
        <div className="inline-flex items-center gap-2 px-3 py-2">
          <span className="font-mono text-[10px] text-muted-foreground/60">{hint.phrase}</span>
          <span className="h-3 w-px shrink-0 bg-border/60" />
          <ComboKeys combo={combos[0]} />
          <span className="h-3 w-px shrink-0 bg-border/60" />
          <DismissButton onDismiss={onDismiss} />
        </div>
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
      className="inline-flex flex-col overflow-hidden rounded-xl border border-border bg-popover/95 shadow-lg backdrop-blur-sm"
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
    >
      {metricsApp && <MetricsNote app={metricsApp} />}
      <div className="flex flex-col gap-1.5 px-3 py-2">
        <div className="flex items-center justify-between gap-3">
          <span className="text-[9px] font-medium uppercase tracking-widest text-muted-foreground/60">
            {combos.length} chords
          </span>
          <DismissButton onDismiss={onDismiss} />
        </div>
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
      </div>
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

function OptionsView({ hint, fadeMs, metricsApp, onMouseEnter, onMouseLeave, onDismiss }: ViewProps) {
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
      className="flex max-h-full w-[300px] flex-col overflow-hidden rounded-xl border border-border bg-popover/97 shadow-xl backdrop-blur-md"
      style={{ transformOrigin: "top left" }}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
    >
      {metricsApp && <MetricsNote app={metricsApp} />}
      {/* Header: word + intent label + dismiss — fixed, never scrolls off */}
      <div className="flex shrink-0 items-baseline justify-between gap-2 border-b border-border/60 px-3 pt-2.5 pb-2">
        <span className="font-mono text-[11px] font-semibold tracking-wide text-foreground/90">
          {hint.phrase}
        </span>
        <div className="flex shrink-0 items-center gap-1.5">
          <span className="rounded-full border border-gold/25 bg-gold/12 px-1.5 py-px text-[9px] font-medium uppercase tracking-widest text-gold/80">
            {hint.source === "suggested"
              ? primary?.swap_target
                ? "swap available"
                : "no chord yet"
              : hint.combos?.[0]?.conflicts.length
                ? "conflict"
                : "alternatives"}
          </span>
          <DismissButton onDismiss={onDismiss} />
        </div>
      </div>

      {/* Primary combo — fixed, never scrolls off */}
      <div className="shrink-0 px-3 pt-2.5 pb-2">
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
          <AddChordButton phrase={hint.phrase} combo={hint.primary_combo} />
        </div>
      </div>

      {/* Alternatives — the only scrolling region; fills remaining height */}
      {hasAlts && (
        <>
          <div className="mx-3 shrink-0 border-t border-border/40" />
          {/* Direct flex child so it gets a definite height from the bounded
              card and scrolls reliably (a nested h-full does not resolve). */}
          <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 pt-2 pb-2.5 space-y-1.5">
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
                <AddChordButton phrase={hint.phrase} combo={alt.combo} />
              </motion.div>
            ))}
          </div>
        </>
      )}
    </motion.div>
  );
}

// ── Coaching surface ──────────────────────────────────────────────────────────

/**
 * Coaching surface. Renders the chord mapping for a just-typed word.
 *
 * Runs in the provider-free overlay root (overlay-main.tsx). The NSPanel is
 * positioned in Rust on `coaching_position`; this surface only renders content
 * and drives the show/fade/dismiss lifecycle. Panel show/hide + interactivity
 * are routed through the panel arbiter (src/lib/overlayPanel.ts) so coaching
 * coexists with other surfaces without tearing down the shared NSPanel.
 */
export function CoachingSurface() {
  const [hint, setHint] = useState<CoachingHint | null>(null);
  const [visible, setVisible] = useState(false);
  // Set from the position event when the focused app is a Chromium browser with
  // Text Metrics off; drives the prompt banner. Reset on each new hint.
  const [metricsApp, setMetricsApp] = useState<string | null>(null);

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
  // Mirror of `visible` readable from the (stale-closure-prone) onExitComplete
  // callback. When one hint REPLACES another, the outgoing hint's exit animation
  // completes while `visible` is still true — we must NOT release the shared
  // NSPanel in that case, or the just-shown replacement hint vanishes ~fade_ms later.
  const visibleRef = useRef(false);
  // Whether this surface currently holds a visibility acquire on the arbiter.
  // Guards against double-acquire across hint replacements (visible stays true).
  const holdsVisibilityRef = useRef(false);

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

  // Keep visibleRef in sync so onExitComplete can distinguish a real dismiss
  // (visible=false) from a hint replacement (visible still true). Also route
  // panel show/interactive through the arbiter while a hint is up: the dismiss
  // button is clickable but the panel stays click-through the rest of the time,
  // and coaching holds a visibility acquire so its auto-hide only tears the
  // panel down when no other surface is using it.
  // Tracks whether we currently hold an interactivity acquire, so the visible
  // toggle acquires/releases exactly once per transition (no double release).
  const holdsInteractiveRef = useRef(false);
  useEffect(() => {
    visibleRef.current = visible;
    if (visible) {
      if (!holdsVisibilityRef.current) {
        acquireVisibility();
        holdsVisibilityRef.current = true;
      }
      if (!holdsInteractiveRef.current) {
        acquireInteractive();
        holdsInteractiveRef.current = true;
      }
    } else if (holdsInteractiveRef.current) {
      holdsInteractiveRef.current = false;
      releaseInteractive();
    }
  }, [visible]);

  // Explicit user dismissal (close button): clear the backend flag so the
  // detector stops tracking this hint, then fade out locally.
  const handleDismiss = () => {
    void coachLog("dismiss button clicked");
    void dismissOverlay();
    clearHideTimer();
    setVisible(false);
  };

  useEffect(() => {
    let mounted = true;
    void coachLog("mount: effect setup");

    getSettings()
      .then((s) => {
        if (!mounted) return;
        if (typeof s.coaching_show_ms === "number") showMsRef.current = s.coaching_show_ms;
        if (typeof s.coaching_fade_ms === "number") fadeMsRef.current = s.coaching_fade_ms;
        if (typeof s.coaching_persist === "boolean") persistRef.current = s.coaching_persist;
        void coachLog(`getSettings resolved persist=${s.coaching_persist} show_ms=${s.coaching_show_ms}`);
      })
      .catch(() => {
        // Fall back to defaults — the overlay still works.
      });

    const dismiss = () => {
      // The backend decides WHEN to send `coaching_dismiss` per mode (every
      // keystroke in normal mode; only when a new word begins in persist mode),
      // so we just honor it here regardless of mode.
      void coachLog("dismiss() invoked (coaching_dismiss event received)");
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
      setMetricsApp(null); // cleared until the position event reports the app
      setVisible(true);
      clearHideTimer();

      const willArm = !persistRef.current && !hoveredRef.current;
      void coachLog(
        `onCoachingHint id=${h.id} persist=${h.persist} persistRef=${persistRef.current} hovered=${hoveredRef.current} arm_timer=${willArm}`,
      );
      // Don't start the hide timer if the pointer is already over the overlay
      // (user hovered before new hint arrived).
      if (willArm) {
        // Hold fully visible for show_ms, then fade out over fade_ms.
        hideTimerRef.current = setTimeout(() => {
          void coachLog(`hide timer fired id=${h.id} after ${showMsRef.current}ms`);
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
      // Chromium browser (Dia/Arc) with Text Metrics off → show the prompt.
      setMetricsApp(p.text_metrics_app ?? null);
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
      void coachLog("unmount: effect cleanup (listeners torn down)");
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
    <AnimatePresence
      onExitComplete={() => {
        // Fade-out finished. Only release the NSPanel visibility hold if we're
        // actually going dark — NOT when this exit was caused by one hint
        // REPLACING another (key change while visible stays true). Releasing on
        // a replacement would let the arbiter tear down the shared panel that
        // the incoming hint just rendered into, making the new hint flash and
        // vanish ~fade_ms after appearing.
        if (visibleRef.current) {
          void coachLog("onExitComplete: replacement (visible) -> keep panel");
          return;
        }
        void coachLog("onExitComplete -> releaseVisibility()");
        if (holdsVisibilityRef.current) {
          holdsVisibilityRef.current = false;
          releaseVisibility();
        }
      }}
    >
      {visible && hint && (
        optionsMode ? (
          <OptionsView
            key={hint.id}
            hint={hint}
            fadeMs={fadeMsRef.current}
            metricsApp={metricsApp}
            onMouseEnter={handleMouseEnter}
            onMouseLeave={handleMouseLeave}
            onDismiss={handleDismiss}
          />
        ) : (
          <ReminderView
            key={hint.id}
            hint={hint}
            fadeMs={fadeMsRef.current}
            metricsApp={metricsApp}
            onMouseEnter={handleMouseEnter}
            onMouseLeave={handleMouseLeave}
            onDismiss={handleDismiss}
          />
        )
      )}
    </AnimatePresence>
  );
}
