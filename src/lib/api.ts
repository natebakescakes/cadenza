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
