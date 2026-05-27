// Tauri command layer. Every command in the contract is defined here and
// registered in `lib.rs`. Bodies call into the (stubbed) modules or return
// stub data. Real logic is filled in by later agents.

use tauri::{Emitter, Manager, State};

use crate::engine;
use crate::serial;
use crate::storage::Storage;
use crate::types::{
    ActivityBlock, BanlistEntry, ChordRecord, DeviceInfo, DeviceSettings, LoggingState,
    PracticeCard, PracticeCardStats, PracticeOverview, Proficiency, SerialPortInfo, Settings,
    Suggestion, WordRecord, WpmSample, WpmSummary,
};
use crate::{AppState, EVT_DEVICE_CHANGED, EVT_LOGGING_STATE};

/// Derive chord_char_threshold_ms and arpeggio_threshold_ms from raw device
/// settings and apply them to the live Settings mutex.
///
/// Formula rationale (documented here for tuning):
///   chord_char_threshold_ms = max(output_delay_us / 1000 * 3, 2)
///     — 3× the per-char emission delay gives headroom for USB polling jitter
///       while staying well below typical human inter-key intervals (>20 ms).
///       Floor of 2 ms prevents the threshold collapsing to near-zero for very
///       fast devices.  Tune the multiplier (3) against [FLUSH] avg_ms logs.
///
///   arpeggio_threshold_ms = max(output_delay_us / 1000 * 6, 8) capped at 15 ms.
///     — The arpeggiate INPUT timeout (0x54, hundreds–thousands of ms) is the
///       window the *user* has to press a modifier; it is NOT the output burst
///       gap seen by the host keylogger.  Arpeggio/compound output arrives as a
///       fast burst identical to a normal chord (empirically ≤ ~5 ms per char).
///       We derive from output_delay_us (6× for a wider but still small window)
///       and cap at 15 ms so manually-typed in-chordmap words (max > 20 ms)
///       are never misclassified.  Real [FLUSH] logs confirm: genuine chord
///       bursts show max ≤ ~5 ms; manual typing of known words shows max > 100 ms.
fn apply_device_thresholds(settings: &mut Settings, ds: &DeviceSettings) {
    if ds.output_delay_us >= 0 {
        let derived_chord_ms = (ds.output_delay_us as f64 / 1000.0) * 3.0;
        settings.chord_char_threshold_ms = derived_chord_ms.max(2.0);

        // Arpeggio threshold: output-burst gate, NOT the input arpeggiate timeout.
        let derived_arp_ms = (ds.output_delay_us as f64 / 1000.0) * 6.0;
        settings.arpeggio_threshold_ms = derived_arp_ms.max(8.0).min(15.0);
    }
    // arpeggiate_timeout_ms is intentionally NOT used here — it is an INPUT
    // window (hundreds of ms) and would cause manual in-chordmap words to be
    // misclassified as chorded.  It is still read and shown in DeviceSettings.
}

/// Emit the current logging state to the frontend.
fn emit_logging_state(state: &AppState) {
    let snapshot = state.logging_state.lock().clone();
    if let Some(app) = state.app_handle.lock().as_ref() {
        let _ = app.emit(EVT_LOGGING_STATE, &snapshot);
    }
}

// --- Dev bypass -----------------------------------------------------------

/// Dev mode = debug build OR env var CADENZA_NO_AUTH=1/true.
fn dev_mode_enabled() -> bool {
    if cfg!(debug_assertions) {
        return true;
    }
    matches!(
        std::env::var("CADENZA_NO_AUTH").as_deref(),
        Ok("1") | Ok("true")
    )
}

// --- Database lifecycle ---------------------------------------------------

#[tauri::command]
pub fn is_db_initialized() -> bool {
    Storage::is_initialized()
}

/// Open the DB without password verification — dev builds only.
/// Returns Err in release builds (unless CADENZA_NO_AUTH is set).
#[tauri::command]
pub fn db_dev_unlock(state: State<'_, AppState>) -> Result<bool, String> {
    if !dev_mode_enabled() {
        return Err("dev unlock disabled in release".to_string());
    }
    let storage = Storage::open_no_auth().map_err(|e| e.to_string())?;
    *state.storage.lock() = Some(storage);
    state.logging_state.lock().db_unlocked = true;
    Ok(true)
}

#[tauri::command]
pub fn db_init(state: State<'_, AppState>, password: String) -> Result<(), String> {
    let storage = Storage::init(&password).map_err(|e| e.to_string())?;
    *state.storage.lock() = Some(storage);
    state.logging_state.lock().db_unlocked = true;
    Ok(())
}

#[tauri::command]
pub fn db_unlock(state: State<'_, AppState>, password: String) -> Result<bool, String> {
    match Storage::unlock(&password) {
        Ok(storage) => {
            *state.storage.lock() = Some(storage);
            state.logging_state.lock().db_unlocked = true;
            Ok(true)
        }
        Err(_) => Ok(false),
    }
}

// --- Settings -------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.lock().clone()
}

#[tauri::command]
pub fn set_settings(state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
    // If the user explicitly changed a detection threshold, disable auto-derive
    // so a subsequent connect/refresh doesn't clobber their custom value.
    let mut incoming = settings.clone();
    {
        let current = state.settings.lock();
        let threshold_changed = incoming.chord_char_threshold_ms != current.chord_char_threshold_ms
            || incoming.arpeggio_threshold_ms != current.arpeggio_threshold_ms;
        if threshold_changed {
            incoming.thresholds_auto = false;
        }
    }
    state.engine.lock().update_settings(incoming.clone());
    *state.settings.lock() = incoming;
    Ok(())
}

// --- Logging --------------------------------------------------------------

/// Frontend → backend log bridge for the overlay webview (its console is not
/// visible in the rolling log). Prefixed `[COACH-FE]` so it interleaves with the
/// backend `[COACH]` lifecycle lines. Temporary diagnostic for the overlay flash.
#[tauri::command]
pub fn coach_log(msg: String) {
    crate::logging::log_line(&format!("[COACH-FE] {}", msg));
}

#[tauri::command]
pub fn start_logging(state: State<'_, AppState>) -> Result<(), String> {
    // DB must be unlocked before logging (detector writes to it).
    if state.storage.lock().is_none() {
        return Err("database not unlocked".to_string());
    }

    let app = state
        .app_handle
        .lock()
        .clone()
        .ok_or_else(|| "app not ready".to_string())?;

    // Populate the chord phrase set from the DB so the detector can do
    // arpeggio/compound-chord lookups without hitting the DB per flush.
    {
        if let Some(s) = state.storage.lock().as_ref() {
            *state.chord_phrases.write() = s.chord_phrase_set();
        }
    }

    // Spawn the detector thread if not already running.
    if state.detector.lock().is_none() {
        let rx = state
            .key_rx
            .lock()
            .clone()
            .ok_or_else(|| "key channel unavailable".to_string())?;
        let handle = engine::spawn(
            rx,
            state.settings.clone(),
            state.chord_phrases.clone(),
            state.device.clone(),
            state.coaching_overlay_visible.clone(),
            state.coaching_hint_seq.clone(),
            state.chordmap_gen.clone(),
            state.practice_active.clone(),
            state.practice_target.clone(),
            app,
        );
        *state.detector.lock() = Some(handle);
    }

    // Start (idempotent) the OS keyboard hook and resume forwarding.
    {
        let mut kl = state.keylogger.lock();
        kl.start(state.key_tx.clone());
        kl.resume();
        let err = kl.last_error.lock().clone();
        crate::logging::log_line(&format!(
            "start_logging: keylogger running={} last_error={:?}",
            kl.is_running(),
            err
        ));
    }

    state.logging_state.lock().logging = true;
    emit_logging_state(&state);
    crate::logging::log_line("start_logging: logging active");
    Ok(())
}

#[tauri::command]
pub fn stop_logging(state: State<'_, AppState>) -> Result<(), String> {
    // Pause the hook (keep it alive) and tear down the detector thread.
    state.keylogger.lock().pause();
    if let Some(mut det) = state.detector.lock().take() {
        det.stop();
    }
    state.logging_state.lock().logging = false;
    emit_logging_state(&state);
    crate::logging::log_line("stop_logging: logging paused, detector stopped");
    Ok(())
}

#[tauri::command]
pub fn logging_status(state: State<'_, AppState>) -> LoggingState {
    state.logging_state.lock().clone()
}

// --- Data queries ---------------------------------------------------------

#[tauri::command]
pub fn list_words(
    state: State<'_, AppState>,
    limit: i64,
    sort_by: String,
    search: String,
) -> Vec<WordRecord> {
    match state.storage.lock().as_ref() {
        Some(s) => s.list_words(limit, &sort_by, &search),
        None => Vec::new(),
    }
}

#[tauri::command]
pub fn list_chords(
    state: State<'_, AppState>,
    limit: i64,
    sort_by: String,
    search: String,
) -> Vec<ChordRecord> {
    match state.storage.lock().as_ref() {
        Some(s) => s.list_chords(limit, &sort_by, &search),
        None => Vec::new(),
    }
}

#[tauri::command]
pub fn get_wpm_summary(state: State<'_, AppState>) -> WpmSummary {
    match state.storage.lock().as_ref() {
        Some(s) => s.wpm_summary(),
        None => WpmSummary::default(),
    }
}

#[tauri::command]
pub fn get_wpm_trend(state: State<'_, AppState>, range: String) -> Vec<WpmSample> {
    match state.storage.lock().as_ref() {
        Some(s) => s.wpm_trend(&range),
        None => Vec::new(),
    }
}

#[tauri::command]
pub fn get_suggestions(state: State<'_, AppState>, limit: i64) -> Vec<Suggestion> {
    let device_id = state
        .device
        .lock()
        .as_ref()
        .map(|d| format!("{}-{}", d.name, d.version))
        .unwrap_or_default();
    match state.storage.lock().as_ref() {
        Some(s) => s.suggestions(limit, &device_id),
        None => Vec::new(),
    }
}

#[tauri::command]
pub fn get_recent_blocks(state: State<'_, AppState>) -> Vec<ActivityBlock> {
    match state.storage.lock().as_ref() {
        Some(s) => s.recent_blocks(),
        None => Vec::new(),
    }
}

#[tauri::command]
pub async fn get_proficiency(state: State<'_, AppState>) -> Result<Vec<Proficiency>, String> {
    // Guard: only proceed if the DB is unlocked (storage present).
    if state.storage.lock().is_none() {
        return Ok(Vec::new());
    }
    // Run the potentially-slow JOIN on a blocking thread so the async
    // executor (and therefore the UI) is not stalled.
    let result = tauri::async_runtime::spawn_blocking(|| {
        match Storage::open() {
            Ok(conn) => Storage::from_connection(conn).proficiency(),
            Err(_) => Vec::new(),
        }
    })
    .await
    .unwrap_or_default();
    Ok(result)
}

// --- Device ---------------------------------------------------------------

#[tauri::command]
pub fn scan_devices() -> Vec<SerialPortInfo> {
    serial::scan_devices()
}

#[tauri::command]
pub fn connect_device(state: State<'_, AppState>, port: String) -> Result<DeviceInfo, String> {
    let (mut device, info) = serial::Device::connect(&port).map_err(|e| e.to_string())?;

    // Read device settings and cache them. Then auto-derive thresholds if the
    // user hasn't opted out by manually editing them.
    let ds = device.read_device_settings();
    *state.device_settings.lock() = Some(ds.clone());
    {
        let mut settings = state.settings.lock();
        if settings.thresholds_auto {
            apply_device_thresholds(&mut settings, &ds);
            crate::logging::log_line(&format!(
                "connect_device: auto-derived thresholds from device \
                 chord_char={:.2}ms arpeggio={:.2}ms (output_delay={}µs arpeggiate_timeout={}ms)",
                settings.chord_char_threshold_ms,
                settings.arpeggio_threshold_ms,
                ds.output_delay_us,
                ds.arpeggiate_timeout_ms,
            ));
        }
    }

    // Read key layout before moving device into the lock — enables joystick-aware
    // chord suggestions immediately without waiting for a full refresh_chordmap.
    let layout = device.read_layout();
    let layout_device_id = device.device_id();

    *state.device.lock() = Some(info.clone());
    *state.device_conn.lock() = Some(device);

    if !layout.is_empty() {
        if let Some(s) = state.storage.lock().as_ref() {
            let _ = s.replace_device_layout(&layout_device_id, layout);
        }
    }

    if let Some(app) = state.app_handle.lock().as_ref() {
        let _ = app.emit(EVT_DEVICE_CHANGED, &info);
    }
    Ok(info)
}

#[tauri::command]
pub fn current_device(state: State<'_, AppState>) -> Option<DeviceInfo> {
    state.device.lock().clone()
}

#[tauri::command]
pub fn refresh_chordmap(state: State<'_, AppState>) -> Result<i64, String> {
    // Read the entire chord map from the connected device, then persist it.
    // Also refresh device settings and re-derive thresholds while we have the
    // serial connection open (same serial lock — do settings first).
    let chords;
    let layout;
    let device_id;
    {
        let mut guard = state.device_conn.lock();
        let device = guard
            .as_mut()
            .ok_or_else(|| "no device connected".to_string())?;
        device_id = device.device_id();

        // Re-read device settings and optionally re-derive thresholds.
        let ds = device.read_device_settings();
        *state.device_settings.lock() = Some(ds.clone());
        {
            let mut settings = state.settings.lock();
            if settings.thresholds_auto {
                apply_device_thresholds(&mut settings, &ds);
            }
        }

        chords = device.read_all_chords().map_err(|e| e.to_string())?;
        layout = device.read_layout();
    }

    let count = chords.len() as i64;
    match state.storage.lock().as_ref() {
        Some(s) => {
            s.replace_device_chords(&device_id, chords)
                .map_err(|e| e.to_string())?;
            if !layout.is_empty() {
                s.replace_device_layout(&device_id, layout)
                    .map_err(|e| e.to_string())?;
            }
            // Rebuild the in-memory phrase set so the live detector picks up
            // the new map immediately without restart.
            *state.chord_phrases.write() = s.chord_phrase_set();
            // Bump the chordmap generation so the detector's cached coaching
            // chord maps (built once per session from this same data) are
            // rebuilt on the next manual word instead of serving stale maps.
            state
                .chordmap_gen
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        None => return Err("database not unlocked".to_string()),
    }
    Ok(count)
}

/// Position + show the shared overlay NSPanel at the current caret, reusing the
/// coaching caret locator and the same center-fallback behavior. AX/AppKit must
/// run on the macOS main thread, so we hop there via GCD (matching the existing
/// coaching `coaching_position` listener / `hide_overlay` pattern). Returns
/// immediately; the locate + show happen on the main queue. macOS-only.
#[tauri::command]
pub fn show_overlay_at_caret(_app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        position_overlay_at_caret(_app);
    }
    Ok(())
}

/// Locate the caret on the main thread and position+show the overlay panel there
/// (center-fallback when no real caret is found). Must be invoked from any
/// thread; it dispatches the AX + AppKit work onto the main queue. macOS-only.
#[cfg(target_os = "macos")]
fn position_overlay_at_caret(app: tauri::AppHandle) {
    dispatch2::DispatchQueue::main().exec_async(move || {
        if let Some(hit) = crate::coaching::locate_caret() {
            crate::coaching::position_and_show(&app, &hit.rect, hit.centered);
        } else {
            crate::logging::log_line(
                "[SYNC] show_overlay_at_caret — locate_caret returned None (AX untrusted?)",
            );
        }
    });
}

/// Kick off a background refresh of the device chord map and return IMMEDIATELY.
/// Mirrors `refresh_chordmap`'s persistence + in-memory updates, but runs the
/// heavy serial read + DB work off the command thread via `spawn_blocking` so the
/// UI never freezes. Surfaces progress through the generic overlay events
/// (kind="sync"): `overlay:show {state:"syncing"}` at start, then
/// `overlay:update {state:"done", count}` on success or
/// `overlay:update {state:"error", message}` on failure. The frontend owns
/// auto-hide; we never emit `overlay:hide`.
#[tauri::command]
pub fn refresh_chords_bg(app: tauri::AppHandle) -> Result<(), String> {
    run_background_refresh(app);
    Ok(())
}

/// Shared background-refresh entry point. Used by both the `refresh_chords_bg`
/// command and the global hotkey handler. Returns immediately; all heavy work
/// runs on the blocking pool.
pub fn run_background_refresh(app: tauri::AppHandle) {
    use std::sync::atomic::Ordering;

    // Acquire shareable AppState handles on the calling thread (cheap Arc clones)
    // BEFORE moving into the blocking closure. `device_conn` and `storage` are
    // plain `Mutex<Option<…>>` (not Arc), so we reach them through the AppHandle
    // inside the blocking task instead — `State` access does not require the
    // command thread. The detector/typing path uses a keylogger thread (not the
    // serial port), so holding `device_conn` during the read is safe.
    let chord_phrases = app.state::<AppState>().chord_phrases.clone();
    let chordmap_gen = app.state::<AppState>().chordmap_gen.clone();
    let settings = app.state::<AppState>().settings.clone();

    // Show the sync surface at the caret and emit the initial "syncing" state.
    #[cfg(target_os = "macos")]
    position_overlay_at_caret(app.clone());
    let _ = app.emit(
        crate::EVT_OVERLAY_SHOW,
        serde_json::json!({ "kind": "sync", "payload": { "state": "syncing" } }),
    );

    let app_for_task = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result: Result<i64, String> = (|| {
            let state = app_for_task.state::<AppState>();

            let chords;
            let layout;
            let device_id;
            {
                let mut guard = state.device_conn.lock();
                let device = guard
                    .as_mut()
                    .ok_or_else(|| "no device connected".to_string())?;
                device_id = device.device_id();

                // Re-read device settings and optionally re-derive thresholds
                // while we hold the serial lock (same order as refresh_chordmap).
                let ds = device.read_device_settings();
                *state.device_settings.lock() = Some(ds.clone());
                {
                    let mut s = settings.lock();
                    if s.thresholds_auto {
                        apply_device_thresholds(&mut s, &ds);
                    }
                }

                chords = device.read_all_chords().map_err(|e| e.to_string())?;
                layout = device.read_layout();
            }

            let count = chords.len() as i64;
            match state.storage.lock().as_ref() {
                Some(s) => {
                    s.replace_device_chords(&device_id, chords)
                        .map_err(|e| e.to_string())?;
                    if !layout.is_empty() {
                        s.replace_device_layout(&device_id, layout)
                            .map_err(|e| e.to_string())?;
                    }
                    // Rebuild the in-memory phrase set so the live detector picks
                    // up the new map immediately, and bump the generation so the
                    // detector rebuilds its cached coaching chord maps.
                    *chord_phrases.write() = s.chord_phrase_set();
                    chordmap_gen.fetch_add(1, Ordering::Relaxed);
                }
                None => return Err("database not unlocked".to_string()),
            }
            Ok(count)
        })();

        match result {
            Ok(count) => {
                let _ = app_for_task.emit(
                    crate::EVT_OVERLAY_UPDATE,
                    serde_json::json!({
                        "kind": "sync",
                        "payload": { "state": "done", "count": count }
                    }),
                );
            }
            Err(message) => {
                crate::logging::log_line(&format!("[SYNC] background refresh failed: {message}"));
                let _ = app_for_task.emit(
                    crate::EVT_OVERLAY_UPDATE,
                    serde_json::json!({
                        "kind": "sync",
                        "payload": { "state": "error", "message": message }
                    }),
                );
            }
        }
    });
}

// --- Banlist --------------------------------------------------------------

#[tauri::command]
pub fn list_banlist(state: State<'_, AppState>) -> Vec<BanlistEntry> {
    match state.storage.lock().as_ref() {
        Some(s) => s.list_banlist(),
        None => Vec::new(),
    }
}

#[tauri::command]
pub fn ban_word(state: State<'_, AppState>, word: String) -> Result<(), String> {
    match state.storage.lock().as_ref() {
        Some(s) => s.ban_word(&word).map_err(|e| e.to_string()),
        None => Err("database not unlocked".to_string()),
    }
}

#[tauri::command]
pub fn unban_word(state: State<'_, AppState>, word: String) -> Result<(), String> {
    match state.storage.lock().as_ref() {
        Some(s) => s.unban_word(&word).map_err(|e| e.to_string()),
        None => Err("database not unlocked".to_string()),
    }
}

// --- Device settings & threshold resync -----------------------------------

/// Returns the last-read raw device settings, or None if not yet connected.
#[tauri::command]
pub fn get_device_settings(state: State<'_, AppState>) -> Option<DeviceSettings> {
    state.device_settings.lock().clone()
}

/// Re-enable auto threshold derivation and immediately re-derive from the
/// cached device settings. No-op if no device settings have been read yet.
#[tauri::command]
pub fn resync_device_thresholds(state: State<'_, AppState>) -> Result<(), String> {
    let ds_opt = state.device_settings.lock().clone();
    match ds_opt {
        None => Err("no device settings cached — connect a device first".to_string()),
        Some(ds) => {
            let mut settings = state.settings.lock();
            settings.thresholds_auto = true;
            apply_device_thresholds(&mut settings, &ds);
            crate::logging::log_line(&format!(
                "resync_device_thresholds: chord_char={:.2}ms arpeggio={:.2}ms",
                settings.chord_char_threshold_ms,
                settings.arpeggio_threshold_ms,
            ));
            Ok(())
        }
    }
}

// --- Hidden words (display filter — logged data preserved) ----------------

#[tauri::command]
pub fn hide_word(state: State<'_, AppState>, word: String) -> Result<(), String> {
    match state.storage.lock().as_ref() {
        Some(s) => s.hide_word(&word).map_err(|e| e.to_string()),
        None => Err("database not unlocked".to_string()),
    }
}

#[tauri::command]
pub fn unhide_word(state: State<'_, AppState>, word: String) -> Result<(), String> {
    match state.storage.lock().as_ref() {
        Some(s) => s.unhide_word(&word).map_err(|e| e.to_string()),
        None => Err("database not unlocked".to_string()),
    }
}

#[tauri::command]
pub fn list_hidden(state: State<'_, AppState>) -> Vec<String> {
    match state.storage.lock().as_ref() {
        Some(s) => s.list_hidden(),
        None => Vec::new(),
    }
}

/// Hide the coaching overlay NSPanel. The frontend calls this after its fade-out
/// completes (the optimization); the backend visible-flag timer is the floor.
/// AppKit must run on the main thread, so we hop there via GCD. macOS-only no-op
/// elsewhere.
#[tauri::command]
pub fn hide_overlay(_app: tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        crate::logging::log_line("[COACH] hide_overlay command (frontend-driven)");
        dispatch2::DispatchQueue::main().exec_async(move || {
            crate::coaching::hide_overlay(&_app);
        });
    }
}

/// Toggle whether the overlay panel accepts mouse input. The panel is built
/// click-through (`ignores_mouse_events`) so it never blocks the app beneath it;
/// we flip it interactive only while a hint is on screen so its dismiss button
/// (and other controls) can be clicked, then back to click-through on hide.
#[tauri::command]
pub fn set_overlay_interactive(_app: tauri::AppHandle, _interactive: bool) {
    #[cfg(target_os = "macos")]
    {
        dispatch2::DispatchQueue::main().exec_async(move || {
            crate::coaching::set_overlay_interactive(&_app, _interactive);
        });
    }
}

/// Clear the backend "overlay visible" flag when the user explicitly dismisses
/// the hint (e.g. the overlay's close button). Keeps the detector's state in
/// sync so it stops emitting per-keystroke dismiss signals for a hint that is
/// already gone. The panel itself is hidden by the frontend fade → `hide_overlay`.
#[tauri::command]
pub fn dismiss_overlay(state: State<'_, AppState>) {
    state
        .coaching_overlay_visible
        .store(false, std::sync::atomic::Ordering::Relaxed);
    crate::logging::log_line("[COACH] dismiss_overlay command (user close button)");
}

// --- Practice hub (spaced-repetition drills) ------------------------------
//
// These commands drive the isolated practice trainer. Statistics live in the
// practice_* tables ONLY; the practice-mode gate in the detector (keyed off
// `practice_active`) guarantees ambient stats are untouched while drilling.

/// Current epoch-ms wall clock. Commands own the timestamp so the frontend never
/// passes one.
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Enter practice mode: set the drill target + flip the detector gate on. While
/// active the detector suppresses ALL ambient writes/emits and emits
/// `practice_chord` instead.
#[tauri::command]
pub fn practice_begin(state: State<'_, AppState>, phrase: String) {
    *state.practice_target.lock() = Some(phrase.clone());
    state
        .practice_active
        .store(true, std::sync::atomic::Ordering::Relaxed);
    crate::logging::log_line(&format!("[PRACTICE] begin target=\"{}\"", phrase));
}

/// Leave practice mode: clear the target + flip the gate off so ambient logging
/// resumes normally.
#[tauri::command]
pub fn practice_end(state: State<'_, AppState>) {
    state
        .practice_active
        .store(false, std::sync::atomic::Ordering::Relaxed);
    *state.practice_target.lock() = None;
    crate::logging::log_line("[PRACTICE] end");
}

#[tauri::command]
pub fn practice_due_count(state: State<'_, AppState>) -> i64 {
    match state.storage.lock().as_ref() {
        Some(s) => s.practice_due_count(now_ms()),
        None => 0,
    }
}

#[tauri::command]
pub fn practice_due_queue(state: State<'_, AppState>, limit: i64) -> Vec<PracticeCard> {
    match state.storage.lock().as_ref() {
        Some(s) => s.practice_due_queue(now_ms(), limit),
        None => Vec::new(),
    }
}

#[tauri::command]
pub fn practice_start_session(state: State<'_, AppState>) -> i64 {
    match state.storage.lock().as_ref() {
        Some(s) => s.practice_start_session(now_ms()),
        None => 0,
    }
}

/// Log a practice attempt AND update its SM-2 card. Also marks the session
/// complete (a result submission ends the drill round for that card).
#[tauri::command]
pub fn practice_submit_result(
    state: State<'_, AppState>,
    session_id: i64,
    phrase: String,
    correct: bool,
    first_try: bool,
    fire_ms: f64,
) -> Result<(), String> {
    let now = now_ms();
    match state.storage.lock().as_ref() {
        Some(s) => {
            s.practice_log_attempt(session_id, &phrase, correct, first_try, fire_ms, now);
            s.practice_submit_result(&phrase, correct, first_try, fire_ms, now);
            Ok(())
        }
        None => Err("database not unlocked".to_string()),
    }
}

#[tauri::command]
pub fn practice_card_stats(state: State<'_, AppState>, phrase: String) -> PracticeCardStats {
    match state.storage.lock().as_ref() {
        Some(s) => s.practice_card_stats(&phrase),
        None => PracticeCardStats {
            phrase,
            ease: 2.5,
            ..PracticeCardStats::default()
        },
    }
}

#[tauri::command]
pub fn practice_overview(state: State<'_, AppState>) -> PracticeOverview {
    match state.storage.lock().as_ref() {
        Some(s) => s.practice_overview(now_ms()),
        None => PracticeOverview::default(),
    }
}

/// Mark a practice session finished (stamps `completed_at`). The streak in
/// `practice_overview` counts days with a completed session, so this must be
/// called once when a drill session ends.
#[tauri::command]
pub fn practice_complete_session(state: State<'_, AppState>, session_id: i64) -> Result<(), String> {
    match state.storage.lock().as_ref() {
        Some(s) => {
            s.practice_complete_session(session_id, now_ms());
            Ok(())
        }
        None => Err("database not unlocked".to_string()),
    }
}
