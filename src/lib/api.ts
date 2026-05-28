// Typed wrappers around the Tauri command + event contract.
// One function per backend command (camelCase) plus event listener helpers.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  ActivityBlock,
  BanlistEntry,
  ChordRecord,
  CoachingHint,
  CoachingPosition,
  DeviceInfo,
  DeviceSettings,
  KeyEvent,
  LoggingState,
  OverlaySurfaceEvent,
  PracticeAttemptSummary,
  PracticeCard,
  PracticeCardStats,
  PracticeChordEvent,
  PracticeOverview,
  Proficiency,
  SerialPortInfo,
  Settings,
  Suggestion,
  WordRecord,
  WpmSample,
  WpmSummary,
} from "./types";

// --- Database lifecycle ---------------------------------------------------

export const isDbInitialized = (): Promise<boolean> =>
  invoke("is_db_initialized");

export const dbInit = (password: string): Promise<void> =>
  invoke("db_init", { password });

export const dbUnlock = (password: string): Promise<boolean> =>
  invoke("db_unlock", { password });

export const dbDevUnlock = (): Promise<boolean> =>
  invoke("db_dev_unlock");

// --- Settings -------------------------------------------------------------

export const getSettings = (): Promise<Settings> => invoke("get_settings");

export const setSettings = (settings: Settings): Promise<void> =>
  invoke("set_settings", { settings });

// --- Logging --------------------------------------------------------------

export const startLogging = (): Promise<void> => invoke("start_logging");

export const stopLogging = (): Promise<void> => invoke("stop_logging");

export const loggingStatus = (): Promise<LoggingState> =>
  invoke("logging_status");

// --- Data queries ---------------------------------------------------------

export const listWords = (
  limit: number,
  sortBy: string,
  search: string,
): Promise<WordRecord[]> =>
  invoke("list_words", { limit, sortBy, search });

export const listChords = (
  limit: number,
  sortBy: string,
  search: string,
): Promise<ChordRecord[]> =>
  invoke("list_chords", { limit, sortBy, search });

export const getWpmSummary = (): Promise<WpmSummary> =>
  invoke("get_wpm_summary");

/** range: "day" | "week" | "month" */
export const getWpmTrend = (range: string): Promise<WpmSample[]> =>
  invoke("get_wpm_trend", { range });

export const getSuggestions = (limit: number): Promise<Suggestion[]> =>
  invoke("get_suggestions", { limit });

export const getRecentBlocks = (): Promise<ActivityBlock[]> =>
  invoke("get_recent_blocks");

export const getProficiency = (): Promise<Proficiency[]> =>
  invoke("get_proficiency");

// --- Device ---------------------------------------------------------------

export const scanDevices = (): Promise<SerialPortInfo[]> =>
  invoke("scan_devices");

export const connectDevice = (port: string): Promise<DeviceInfo> =>
  invoke("connect_device", { port });

export const currentDevice = (): Promise<DeviceInfo | null> =>
  invoke("current_device");

export const refreshChordmap = (): Promise<number> =>
  invoke("refresh_chordmap");

export const getDeviceSettings = (): Promise<DeviceSettings | null> =>
  invoke("get_device_settings");

export const resyncDeviceThresholds = (): Promise<void> =>
  invoke("resync_device_thresholds");

// --- Banlist --------------------------------------------------------------

export const listBanlist = (): Promise<BanlistEntry[]> =>
  invoke("list_banlist");

export const banWord = (word: string): Promise<void> =>
  invoke("ban_word", { word });

export const unbanWord = (word: string): Promise<void> =>
  invoke("unban_word", { word });

// --- Hidden words (display filter) ----------------------------------------

export const hideWord = (word: string): Promise<void> =>
  invoke("hide_word", { word });

export const unhideWord = (word: string): Promise<void> =>
  invoke("unhide_word", { word });

export const listHidden = (): Promise<string[]> =>
  invoke("list_hidden");

// --- Coaching overlay -----------------------------------------------------

/** Hide the coaching overlay NSPanel (called after the fade-out completes). */
export const hideOverlay = (): Promise<void> => invoke("hide_overlay");

/** Flip the overlay between click-through and interactive (clickable controls). */
export const setOverlayInteractive = (interactive: boolean): Promise<void> =>
  invoke<void>("set_overlay_interactive", { interactive }).catch(() => undefined);

/** Clear the backend visible flag when the user explicitly dismisses the hint. */
export const dismissOverlay = (): Promise<void> =>
  invoke<void>("dismiss_overlay").catch(() => undefined);

/** Temporary diagnostic: route overlay-webview lifecycle into the backend log. */
export const coachLog = (msg: string): Promise<void> =>
  invoke<void>("coach_log", { msg }).catch(() => undefined);

// --- Overlay surface framework --------------------------------------------

/** Show + caret-anchor the shared overlay NSPanel (used by non-coaching surfaces). */
export const showOverlayAtCaret = (): Promise<void> =>
  invoke<void>("show_overlay_at_caret").catch(() => undefined);

/** Kick off a background chord-library refresh (drives the `sync` surface). */
export const refreshChordsBg = (): Promise<void> =>
  invoke<void>("refresh_chords_bg").catch(() => undefined);

// --- Practice hub (spaced-repetition chord drill) -------------------------

/** Enter practice mode for a target phrase (engine starts watching for its chord). */
export const practiceBegin = (phrase: string): Promise<void> =>
  invoke("practice_begin", { phrase });

/** Leave practice mode. ALWAYS call on unmount/leave so the engine doesn't stay in practice. */
export const practiceEnd = (): Promise<void> => invoke("practice_end");

/** Count of cards currently due (existing due + new seed candidates). */
export const practiceDueCount = (): Promise<number> =>
  invoke("practice_due_count");

/** The due queue, capped at `limit` cards. */
export const practiceDueQueue = (limit: number): Promise<PracticeCard[]> =>
  invoke("practice_due_queue", { limit });

/** An alternative queue: a random sample of the whole device chord library. */
export const practiceAllQueue = (limit: number): Promise<PracticeCard[]> =>
  invoke("practice_all_queue", { limit });

/** Begin a practice session; returns the session id used for result submission. */
export const practiceStartSession = (): Promise<number> =>
  invoke("practice_start_session");

/** Record one attempt's result against the session. */
export const practiceSubmitResult = (
  sessionId: number,
  phrase: string,
  correct: boolean,
  firstTry: boolean,
  fireMs: number,
  backspaces: number,
  corrections: number,
  hintUsed: boolean,
): Promise<void> =>
  invoke("practice_submit_result", {
    sessionId,
    phrase,
    correct,
    firstTry,
    fireMs,
    backspaces,
    corrections,
    hintUsed,
  });

/** Per-card practice statistics (SM-2 state + recent speed + first-try accuracy). */
export const practiceCardStats = (phrase: string): Promise<PracticeCardStats> =>
  invoke("practice_card_stats", { phrase });

/** Per-card stats for every drilled chord (for the "your chords" stats view). */
export const practiceAllCardStats = (): Promise<PracticeCardStats[]> =>
  invoke("practice_all_card_stats");

/** Aggregate practice overview for the hub header (totals, streak, due count). */
export const practiceOverview = (): Promise<PracticeOverview> =>
  invoke("practice_overview");

/** Mark a session finished (stamps completed_at — required for streak counting). */
export const practiceCompleteSession = (sessionId: number): Promise<void> =>
  invoke<void>("practice_complete_session", { sessionId }).catch(() => undefined);

/** Per-word recap of a completed session (one row per logged attempt). */
export const practiceSessionSummary = (
  sessionId: number,
): Promise<PracticeAttemptSummary[]> =>
  invoke("practice_session_summary", { sessionId });

// --- Event listeners ------------------------------------------------------

export const onKeystroke = (
  cb: (e: KeyEvent) => void,
): Promise<UnlistenFn> =>
  listen<KeyEvent>("keystroke", (event) => cb(event.payload));

export const onCoachingHint = (
  cb: (e: CoachingHint) => void,
): Promise<UnlistenFn> =>
  listen<CoachingHint>("coaching_hint", (event) => cb(event.payload));

export const onCoachingPosition = (
  cb: (e: CoachingPosition) => void,
): Promise<UnlistenFn> =>
  listen<CoachingPosition>("coaching_position", (event) => cb(event.payload));

/**
 * Empty-payload dismiss signal: fires on the next keystroke while the overlay
 * is visible, and again from the backend clear-timer floor. Privacy-safe — it
 * carries NO typed character, only the "a key happened" / "time's up" trigger.
 */
export const onCoachingDismiss = (cb: () => void): Promise<UnlistenFn> =>
  listen<void>("coaching_dismiss", () => cb());

// Generic surface events — mirror the coaching helpers. `overlay:show` mounts a
// surface, `overlay:update` re-renders it with a new payload, `overlay:hide`
// removes it. Coaching ignores these and keeps its dedicated coaching_* events.
export const onOverlayShow = (
  cb: (e: OverlaySurfaceEvent) => void,
): Promise<UnlistenFn> =>
  listen<OverlaySurfaceEvent>("overlay:show", (event) => cb(event.payload));

export const onOverlayUpdate = (
  cb: (e: OverlaySurfaceEvent) => void,
): Promise<UnlistenFn> =>
  listen<OverlaySurfaceEvent>("overlay:update", (event) => cb(event.payload));

export const onOverlayHide = (
  cb: (e: { kind: string }) => void,
): Promise<UnlistenFn> =>
  listen<{ kind: string }>("overlay:hide", (event) => cb(event.payload));

/**
 * Real chord-fire during a practice drill. The backend detects the actual chord
 * and emits this for the current target; the UI advances the drill on receipt.
 */
export const onPracticeChord = (
  cb: (e: PracticeChordEvent) => void,
): Promise<UnlistenFn> =>
  listen<PracticeChordEvent>("practice_chord", (event) => cb(event.payload));

export const onWpm = (cb: (e: WpmSample) => void): Promise<UnlistenFn> =>
  listen<WpmSample>("wpm", (event) => cb(event.payload));

export const onWordLogged = (
  cb: (e: WordRecord) => void,
): Promise<UnlistenFn> =>
  listen<WordRecord>("word_logged", (event) => cb(event.payload));

export const onChordLogged = (
  cb: (e: ChordRecord) => void,
): Promise<UnlistenFn> =>
  listen<ChordRecord>("chord_logged", (event) => cb(event.payload));

export const onLoggingState = (
  cb: (e: LoggingState) => void,
): Promise<UnlistenFn> =>
  listen<LoggingState>("logging_state", (event) => cb(event.payload));

export const onDeviceChanged = (
  cb: (e: DeviceInfo | null) => void,
): Promise<UnlistenFn> =>
  listen<DeviceInfo | null>("device_changed", (event) => cb(event.payload));
