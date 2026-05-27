// TypeScript mirror of the Rust serde types in `src-tauri/src/types.rs`.
// Keep these in sync with the backend contract.

export interface Modifiers {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  meta: boolean;
}

export interface KeyEvent {
  code: string;
  key: string;
  pressed: boolean;
  modifiers: Modifiers;
  ts_ms: number;
}

export interface WordRecord {
  word: string;
  frequency: number;
  last_used: number;
  avg_speed_ms: number;
  score: number;
  /** clean_count / frequency — fraction of occurrences with zero backspace corrections */
  accuracy_rate: number;
}

export interface ChordRecord {
  phrase: string;
  frequency: number;
  last_used: number;
  avg_speed_ms: number;
  /** "chord" (simultaneous burst) | "arpeggio" (sequential burst, in chordmap) */
  kind: string;
}

/** source: "overall" | "chorded" | "manual" */
export interface WpmSample {
  t: number;
  wpm: number;
  source: string;
}

export interface WpmSummary {
  rolling: number;
  session: number;
  overall: number;
  chorded: number;
  manual: number;
}

export interface ChordCombo {
  /** "chord" or "compound" */
  kind: string;
  /** For chord: ["a + h + t"]. For compound: ["h + i + s", "a + l"]. */
  parts: string[];
  /** Existing chord phrases using the same key combination. */
  conflicts: string[];
}

export interface Suggestion {
  phrase: string;
  frequency: number;
  score: number;
  avg_manual_ms: number;
  projected_saving_ms: number;
  combos: ChordCombo[];
}

export interface Proficiency {
  phrase: string;
  usage_rate: number;
  fired_count: number;
  manual_count: number;
  avg_fire_ms: number;
  consistency: number;
  mastered: boolean;
  /** High-confidence errors: chord fired then same phrase manually retyped within 5s. */
  error_count: number;
  /** error_count / (fired_count + error_count) */
  error_rate: number;
  /** Lower-confidence: chord fired then N backstrokes deleted it within 3s. May include intentional edits. */
  deletion_count: number;
  /** deletion_count / (fired_count + deletion_count) */
  deletion_rate: number;
  /** Chord deleted then a different chord fired within the confusion window. */
  confusion_count: number;
  /** confusion_count / (fired_count + confusion_count) */
  confusion_rate: number;
  /** Human-readable key combinations, one per device_chords row. E.g. ["p + t"]. Empty if no mapping found. */
  combos: string[];
}

/** One candidate key combination in a coaching hint, with conflict info. */
export interface CoachingCombo {
  /** Display string for the combo, e.g. "w + o" or "h + i + s → a + l". */
  combo: string;
  /** Words that already occupy this key combination (empty = no conflict). */
  conflicts: string[];
  /** When occupied, the holder word whose chord we'd suggest reassigning to the
   *  current word (the weakest-used holder). null/undefined = not a swap.
   *  Recommend-only — the app never writes to the device. */
  swap_target?: string | null;
  /** Human-readable rationale, e.g. `you type "race" 12× · "rce" chord fires 1×`. */
  swap_reason?: string | null;
}

/** Coaching overlay hint, emitted immediately on `manual` classification. */
export interface CoachingHint {
  id: number;
  phrase: string;
  primary_combo: string;
  alt_count: number;
  /** "device" | "suggested" */
  source: string;
  /** All candidate combos (primary first), with per-combo conflict lists. */
  combos: CoachingCombo[];
  /** Live settings snapshot at emit time (the overlay window can't see later
   *  Settings edits, so it reads these per-hint rather than once on mount). */
  persist: boolean;
  show_ms: number;
  fade_ms: number;
}

/**
 * Generic overlay-surface event payload. The overlay container is a small
 * surface framework: `kind` routes to a registered surface component, `payload`
 * is that surface's own (opaque-here) data. Coaching keeps its dedicated
 * `coaching_*` events; these drive every OTHER surface (sync, future menus).
 */
export interface OverlaySurfaceEvent {
  kind: string;
  payload: unknown;
}

/** Payload for the `kind: "sync"` surface (chord-library refresh progress). */
export interface SyncSurfacePayload {
  state: "syncing" | "done" | "error";
  count?: number;
  message?: string;
}

/** A screen rectangle in Tauri logical (NS, top-left origin) coords. */
export interface ScreenRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Coaching overlay caret position, emitted by the main-thread AX closure. */
export interface CoachingPosition {
  id: number;
  rect: ScreenRect;
  /** Set when the focused app is a Chromium browser (e.g. Dia, Arc) with Text
   *  Metrics accessibility disabled, so no caret geometry is available. The
   *  overlay shows a prompt to enable it. Absent when a real caret was found. */
  text_metrics_app?: string | null;
}

export interface DeviceInfo {
  name: string;
  company: string;
  device: string;
  chipset: string;
  version: string;
  port: string;
  chord_count: number;
}

export interface SerialPortInfo {
  port: string;
  name: string;
}

export interface Settings {
  new_word_threshold_s: number;
  chord_char_threshold_ms: number;
  allowed_chars: string;
  /** Max ms between any two consecutive chars for a chordmap phrase to be classified as chorded (arpeggio gate). */
  arpeggio_threshold_ms: number;
  /** When true, chord_char_threshold_ms and arpeggio_threshold_ms are auto-derived from device settings on connect/refresh. */
  thresholds_auto: boolean;
  /** Time window (ms) after a chord deletion within which firing a different chord is logged as a confusion event. */
  chord_confusion_window_ms: number;
  /** Master toggle for the real-time chord coaching overlay. */
  coaching_enabled: boolean;
  /** How long (ms) the coaching overlay stays fully visible before fading. */
  coaching_show_ms: number;
  /** Fade-out duration (ms) of the coaching overlay. */
  coaching_fade_ms: number;
  /** Minimum manual word frequency before a suggested (chordless) combo is shown. */
  coaching_suggest_min_count: number;
  /** Minimum phrase length before a suggested (chordless) combo is offered. */
  coaching_suggest_min_len: number;
  /** A previously-mastered chord whose usage_rate drops below this is re-surfaced. */
  coaching_resurface_rate: number;
  /** When true, the overlay stays until the next word (no auto-fade; clears on next word). */
  coaching_persist: boolean;
  /** When true, suppress reminders for already-mastered chords. Default false (show all). */
  coaching_hide_mastered: boolean;
}

/** Raw device settings read via VAR B1 queries. Fields are -1 when the query failed. */
export interface DeviceSettings {
  /** Keyboard output delay in µs (id 0x17). */
  output_delay_us: number;
  /** Arpeggiate timeout in ms (id 0x54). */
  arpeggiate_timeout_ms: number;
  arpeggiate_enabled: boolean;
  chord_press_tolerance_ms: number;
  chord_release_tolerance_ms: number;
  auto_delete_timeout_ms: number;
  chording_enabled: boolean;
  spurring_enabled: boolean;
}

export interface LoggingState {
  logging: boolean;
  db_unlocked: boolean;
}

export interface BanlistEntry {
  word: string;
  added: number;
}

/**
 * A spaced-repetition practice card from the practice queue. Mirrors the Rust
 * `PracticeCard` (snake_case serde — no rename_all). `is_new` marks a freshly
 * seeded weak chord with no SM-2 row yet (card fields are SM-2 defaults).
 */
export interface PracticeCard {
  phrase: string;
  /** Human-readable device key combinations (one per device_chords row). Empty if unmapped. */
  combos: string[];
  ease: number;
  interval_days: number;
  due_at: number;
  reps: number;
  lapses: number;
  last_reviewed: number;
  /** True when this is a freshly-seeded weak chord with no card row yet. */
  is_new: boolean;
}

/** Per-card practice statistics for the detail view. Mirrors Rust `PracticeCardStats`. */
export interface PracticeCardStats {
  phrase: string;
  reps: number;
  lapses: number;
  ease: number;
  interval_days: number;
  due_at: number;
  /** Mean fire_ms over the most recent attempts (0 if none). */
  recent_avg_fire_ms: number;
  /** first_try-correct attempts / total attempts (0.0 if none). */
  first_try_accuracy: number;
}

/** Aggregate practice overview for the hub header. Mirrors Rust `PracticeOverview`. */
export interface PracticeOverview {
  /** Total practice attempts logged across all sessions. */
  total_reps: number;
  /** Distinct phrases that have a practice_cards row. */
  distinct_cards: number;
  /** Consecutive days (ending today) with >=1 completed session. */
  current_streak: number;
  /** Cards currently due (existing due + brand-new seed candidates). */
  due_count: number;
}

/**
 * Real chord-fire event during a practice drill. The backend detects the actual
 * chord and emits this for the current target — the UI never detects chording.
 */
export interface PracticeChordEvent {
  phrase: string;
  fire_ms: number;
  correct: boolean;
}

/** One 5-minute activity block from get_recent_blocks. */
export interface ActivityBlock {
  t: number;
  wpm: number;
  manual_words: string[];
  chorded_words: string[];
  arpeggio_words: string[];
}
