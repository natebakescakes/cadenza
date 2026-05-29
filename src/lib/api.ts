// Typed wrappers around the Tauri command + event contract.
// One function per backend command (camelCase) plus event listener helpers.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  ActivityBlock,
  BanlistEntry,
  ChordRecord,
  ChordRecommendation,
  CoachingHint,
  CoachingPosition,
  DebugChordDump,
  DeviceInfo,
  DeviceSettings,
  KeyEvent,
  LoggingState,
  ModelDownloadProgress,
  ModelEntry,
  OverlaySurfaceEvent,
  PracticeAttemptSummary,
  PracticeCard,
  PracticeCardStats,
  PracticeChordEvent,
  PracticeOverview,
  Proficiency,
  SentenceToken,
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

/** DEBUG (temporary): dump RAW unparsed `CML C1` chord data. `search` is an
 *  optional case-insensitive phrase filter (empty = all chords). */
export const debugDumpChords = (search: string): Promise<DebugChordDump[]> =>
  invoke("debug_dump_chords", { search });

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

// --- Chords-to-add recommendation queue -----------------------------------

/** Add (phrase, combo) to the manually-curated "chords to add" queue. UPSERT —
 *  re-adding the same pair just bumps its timestamp. Recommend-only; never
 *  writes to the device. */
export const addChordRecommendation = (
  phrase: string,
  combo: string,
): Promise<void> =>
  invoke<void>("add_chord_recommendation", { phrase, combo }).catch(
    () => undefined,
  );

export const listChordRecommendations = (): Promise<ChordRecommendation[]> =>
  invoke("list_chord_recommendations");

export const removeChordRecommendation = (
  phrase: string,
  combo: string,
): Promise<void> =>
  invoke<void>("remove_chord_recommendation", { phrase, combo }).catch(
    () => undefined,
  );

export const clearChordRecommendations = (): Promise<void> =>
  invoke<void>("clear_chord_recommendations").catch(() => undefined);

/** Fires after any add/remove/clear so open windows can refresh their list. */
export const onRecommendationsChanged = (cb: () => void): Promise<UnlistenFn> =>
  listen<void>("recommendations_changed", () => cb());

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

/**
 * Generate a natural practice sentence built from library words + glue, via the
 * staged local LLM. `size` picks the target length (S/M/L). Rejects with
 * "Sentence model not set up" when the binary or model is missing — callers
 * should surface a friendly message, not crash.
 */
export const generateSentence = (
  size: "S" | "M" | "L",
): Promise<SentenceToken[]> => invoke("generate_sentence", { size });

// --- Sentence-mode model management ---------------------------------------

/** The model catalog with per-model download/active status. */
export const listModels = (): Promise<ModelEntry[]> => invoke("list_models");

/** Stream-download a catalog model. Progress arrives via `onModelDownloadProgress`. */
export const downloadModel = (id: string): Promise<void> =>
  invoke("download_model", { id });

/** Activate a downloaded model for Sentence mode. */
export const setActiveModel = (id: string): Promise<void> =>
  invoke("set_active_model", { id });

/** Delete (discard) a downloaded model. Clears active if it was the active one. */
export const deleteModel = (id: string): Promise<void> =>
  invoke("delete_model", { id });

/** Whether a usable Sentence-mode model is installed (managed or legacy staged). */
export const sentenceModelReady = (): Promise<boolean> =>
  invoke("sentence_model_ready");

/** Download + extract the Sentence-mode runtime (llama binary + dylibs). Progress
 *  arrives via `onModelDownloadProgress` with `id === "runtime"`. */
export const downloadRuntime = (): Promise<void> => invoke("download_runtime");

/** Whether the Sentence-mode runtime (the llama binary) is installed. */
export const runtimeReady = (): Promise<boolean> => invoke("runtime_ready");

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

/** Throttled Sentence-mode model download progress (received/total + done/error). */
export const onModelDownloadProgress = (
  cb: (e: ModelDownloadProgress) => void,
): Promise<UnlistenFn> =>
  listen<ModelDownloadProgress>("model_download_progress", (event) =>
    cb(event.payload),
  );
