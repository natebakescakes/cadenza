// Tauri command layer. Every command in the contract is defined here and
// registered in `lib.rs`. Bodies call into the (stubbed) modules or return
// stub data. Real logic is filled in by later agents.

use tauri::{Emitter, Manager, State};

use crate::engine;
use crate::serial;
use crate::storage::Storage;
use crate::types::{
    ActivityBlock, BanlistEntry, ChordRecord, DebugChordDump, DeviceInfo, DeviceSettings,
    LoggingState, ModelDownloadProgress, ModelEntry,
    PracticeAttemptSummary, PracticeCard, PracticeCardStats, PracticeOverview, Proficiency,
    SentenceToken, SerialPortInfo, Settings,
    Suggestion, WordRecord, WpmSample, WpmSummary,
};
use crate::{AppState, EVT_DEVICE_CHANGED, EVT_LOGGING_STATE, EVT_MODEL_DOWNLOAD_PROGRESS};

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

/// DEBUG (temporary): dump the RAW, unparsed `CML C1` chord data from the
/// connected device so compound-chord encoding can be reverse-engineered. Locks
/// the serial connection (same pattern as `refresh_chordmap`); `search` is an
/// optional case-insensitive phrase substring filter (empty = all chords). Sync
/// is fine — this is a manual one-shot dev tool, not a hot path.
#[tauri::command]
pub fn debug_dump_chords(
    state: State<'_, AppState>,
    search: String,
) -> Result<Vec<DebugChordDump>, String> {
    let mut guard = state.device_conn.lock();
    let device = guard
        .as_mut()
        .ok_or_else(|| "no device connected".to_string())?;
    let rows = device.dump_chords_raw(&search).map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(index, phrase, actions_hex, phrase_hex)| DebugChordDump {
            index,
            phrase,
            actions_hex,
            phrase_hex,
        })
        .collect())
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

// Heavy practice reads run proficiency() (a slow LOWER-join). They MUST run on
// the blocking pool with their own connection — as sync commands they execute
// on the main thread and freeze the UI (WAL lets a fresh read connection run
// concurrently with writes on the shared one). Mirrors `get_proficiency`.
#[tauri::command]
pub async fn practice_due_count(state: State<'_, AppState>) -> Result<i64, String> {
    if state.storage.lock().is_none() {
        return Ok(0);
    }
    let now = now_ms();
    let result = tauri::async_runtime::spawn_blocking(move || match Storage::open() {
        Ok(conn) => Storage::from_connection(conn).practice_due_count(now),
        Err(_) => 0,
    })
    .await
    .unwrap_or_default();
    Ok(result)
}

#[tauri::command]
pub async fn practice_due_queue(
    state: State<'_, AppState>,
    limit: i64,
) -> Result<Vec<PracticeCard>, String> {
    if state.storage.lock().is_none() {
        return Ok(Vec::new());
    }
    let now = now_ms();
    let result = tauri::async_runtime::spawn_blocking(move || match Storage::open() {
        Ok(conn) => Storage::from_connection(conn).practice_due_queue(now, limit),
        Err(_) => Vec::new(),
    })
    .await
    .unwrap_or_default();
    Ok(result)
}

#[tauri::command]
pub async fn practice_all_queue(
    state: State<'_, AppState>,
    limit: i64,
) -> Result<Vec<PracticeCard>, String> {
    if state.storage.lock().is_none() {
        return Ok(Vec::new());
    }
    let now = now_ms();
    let result = tauri::async_runtime::spawn_blocking(move || match Storage::open() {
        Ok(conn) => Storage::from_connection(conn).practice_all_queue(now, limit),
        Err(_) => Vec::new(),
    })
    .await
    .unwrap_or_default();
    Ok(result)
}

#[tauri::command]
pub async fn practice_session_summary(
    state: State<'_, AppState>,
    session_id: i64,
) -> Result<Vec<PracticeAttemptSummary>, String> {
    if state.storage.lock().is_none() {
        return Ok(Vec::new());
    }
    let result = tauri::async_runtime::spawn_blocking(move || match Storage::open() {
        Ok(conn) => Storage::from_connection(conn).practice_session_summary(session_id),
        Err(_) => Vec::new(),
    })
    .await
    .unwrap_or_default();
    Ok(result)
}

/// Per-card stats for every drilled phrase, for the "your chords" stats view.
/// Runs on the blocking pool with its own read connection (mirrors
/// `practice_session_summary`) — each `practice_card_stats` call is several
/// small aggregate reads, so we keep them off the main thread.
#[tauri::command]
pub async fn practice_all_card_stats(
    state: State<'_, AppState>,
) -> Result<Vec<PracticeCardStats>, String> {
    if state.storage.lock().is_none() {
        return Ok(Vec::new());
    }
    let result = tauri::async_runtime::spawn_blocking(move || match Storage::open() {
        Ok(conn) => Storage::from_connection(conn).practice_all_card_stats(),
        Err(_) => Vec::new(),
    })
    .await
    .unwrap_or_default();
    Ok(result)
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
    backspaces: i64,
    corrections: i64,
    hint_used: bool,
) -> Result<(), String> {
    let now = now_ms();
    match state.storage.lock().as_ref() {
        Some(s) => {
            s.practice_log_attempt(
                session_id,
                &phrase,
                correct,
                first_try,
                fire_ms,
                backspaces,
                corrections,
                hint_used,
                now,
            );
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
pub async fn practice_overview(state: State<'_, AppState>) -> Result<PracticeOverview, String> {
    if state.storage.lock().is_none() {
        return Ok(PracticeOverview::default());
    }
    let now = now_ms();
    let result = tauri::async_runtime::spawn_blocking(move || match Storage::open() {
        Ok(conn) => Storage::from_connection(conn).practice_overview(now),
        Err(_) => PracticeOverview::default(),
    })
    .await
    .unwrap_or_default();
    Ok(result)
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

/// Cheap process-local pseudo-random u64 mixed from the monotonic clock. Avoids
/// pulling in a new RNG dependency; used only to vary the llama seed + pick a
/// random library word for sentence variety (no cryptographic requirement).
fn quick_rand() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    // splitmix64 finalizer for decent bit dispersion from a clock-derived seed.
    let mut z = nanos.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

// --- Sentence-mode model management ---------------------------------------
//
// A small catalog of single-file GGUF models the user can download in-app for
// the local-LLM Sentence practice mode. Downloads stream to `<file>.part` with
// throttled progress events, then atomically rename into place. The legacy
// already-staged `llm/model.gguf` keeps working as a fallback.

/// List the model catalog with per-model download/active status.
#[tauri::command]
pub fn list_models(state: State<'_, AppState>) -> Vec<ModelEntry> {
    let settings = state.settings.lock().clone();
    let active = crate::sentence::active_model_id(&settings);
    crate::sentence::MODEL_CATALOG
        .iter()
        .map(|m| ModelEntry {
            id: m.id.to_string(),
            name: m.name.to_string(),
            description: m.description.to_string(),
            size_mb: m.size_mb,
            downloaded: crate::sentence::is_model_downloaded(m.id),
            active: m.id == active,
        })
        .collect()
}

/// Whether Sentence mode is fully ready: BOTH the runtime binary is installed AND
/// a usable model is resolvable (active catalog model OR the legacy staged
/// `model.gguf`). A model present without the runtime reads as NOT ready.
#[tauri::command]
pub fn sentence_model_ready(state: State<'_, AppState>) -> bool {
    let settings = state.settings.lock().clone();
    crate::sentence::runtime_installed() && crate::sentence::active_model_path(&settings).is_some()
}

/// Whether the Sentence-mode runtime (the `llama-completion` binary + dylibs) has
/// been downloaded/installed. Independent of any model.
#[tauri::command]
pub fn runtime_ready(_state: State<'_, AppState>) -> bool {
    crate::sentence::runtime_installed()
}

/// Set the active Sentence-mode model. Requires the model to be downloaded.
/// Persists via the in-memory settings mutex (the app's settings-save mechanism).
#[tauri::command]
pub fn set_active_model(state: State<'_, AppState>, id: String) -> Result<(), String> {
    if crate::sentence::model_by_id(&id).is_none() {
        return Err(format!("unknown model id: {id}"));
    }
    if !crate::sentence::is_model_downloaded(&id) {
        return Err("model not downloaded".to_string());
    }
    let mut settings = state.settings.lock();
    settings.sentence_model = id;
    state.engine.lock().update_settings(settings.clone());
    Ok(())
}

/// Delete a downloaded model's file. If it was the explicitly-selected active
/// model, clear `sentence_model` so it falls back to the default/another model.
#[tauri::command]
pub fn delete_model(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let path = crate::sentence::model_file(&id).ok_or_else(|| format!("unknown model id: {id}"))?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("could not delete model: {e}"))?;
    }
    let mut settings = state.settings.lock();
    if settings.sentence_model == id {
        settings.sentence_model = String::new();
        state.engine.lock().update_settings(settings.clone());
    }
    Ok(())
}

/// Stream-download a catalog model into `models_dir()`, emitting throttled
/// `model_download_progress` events. Writes to `<filename>.part`, fsyncs +
/// renames to the final name on success, and (if no model is yet selected) sets
/// it active. Cleans up the partial file and emits an `error` payload on failure.
#[tauri::command]
pub async fn download_model(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use std::io::Write;

    let model = crate::sentence::model_by_id(&id)
        .ok_or_else(|| format!("unknown model id: {id}"))?;
    let url = model.url;

    let dir = crate::sentence::models_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create models dir: {e}"))?;
    let final_path = dir.join(model.filename);
    let part_path = dir.join(format!("{}.part", model.filename));

    // Helper: emit a progress event (best-effort).
    let emit_progress = |received: u64, total: u64, done: bool, error: Option<String>| {
        let _ = app.emit(
            EVT_MODEL_DOWNLOAD_PROGRESS,
            ModelDownloadProgress {
                id: id.clone(),
                received,
                total,
                done,
                error,
            },
        );
    };

    // Helper: clean up the partial file, emit an error, and return Err.
    let fail = |msg: String| -> Result<(), String> {
        let _ = std::fs::remove_file(&part_path);
        emit_progress(0, 0, false, Some(msg.clone()));
        Err(msg)
    };

    // reqwest is built with `rustls-no-provider`, so a process-default rustls
    // CryptoProvider must be installed before the first TLS handshake. Install
    // ring (idempotent — Err means another component already set one, which is
    // fine). Without this, building a TLS client would panic at runtime.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let resp = match reqwest::Client::new().get(url).send().await {
        Ok(r) => r,
        Err(e) => return fail(format!("request failed: {e}")),
    };
    if !resp.status().is_success() {
        return fail(format!("download failed: HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);

    let mut file = match std::fs::File::create(&part_path) {
        Ok(f) => f,
        Err(e) => return fail(format!("could not create file: {e}")),
    };

    let mut received: u64 = 0;
    // Throttle progress to at most ~1MB or ~1% of total, whichever is smaller, so
    // the event stream stays cheap on large downloads.
    let step = if total > 0 {
        (total / 100).clamp(1, 1_048_576)
    } else {
        1_048_576
    };
    let mut next_emit: u64 = 0;
    emit_progress(0, total, false, None);

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let bytes = match chunk {
            Ok(b) => b,
            Err(e) => return fail(format!("stream error: {e}")),
        };
        if let Err(e) = file.write_all(&bytes) {
            return fail(format!("write error: {e}"));
        }
        received += bytes.len() as u64;
        if received >= next_emit {
            emit_progress(received, total, false, None);
            next_emit = received + step;
        }
    }

    // Flush + fsync before the atomic rename so a crash can't leave a torn final.
    if let Err(e) = file.flush().and_then(|_| file.sync_all()) {
        return fail(format!("fsync error: {e}"));
    }
    drop(file);
    if let Err(e) = std::fs::rename(&part_path, &final_path) {
        return fail(format!("rename error: {e}"));
    }

    // If no model is selected yet, make this the active one.
    {
        let mut settings = state.settings.lock();
        if settings.sentence_model.trim().is_empty() {
            settings.sentence_model = id.clone();
            state.engine.lock().update_settings(settings.clone());
        }
    }

    emit_progress(received, total, true, None);
    Ok(())
}

/// Stream-download the Sentence-mode runtime tarball into `llm_dir()`, then
/// extract it (system `tar`), make the binary executable, clear quarantine, and
/// delete the archive. Mirrors `download_model`'s streaming exactly (rustls ring
/// install, reqwest stream, `.part` → fsync → rename, throttled progress), but all
/// progress events carry `id: "runtime"` so the frontend's id-keyed listener
/// handles them. Cleans up + emits an `error` payload on any failure.
#[tauri::command]
pub async fn download_runtime(
    app: tauri::AppHandle,
    _state: State<'_, AppState>,
) -> Result<(), String> {
    use futures_util::StreamExt;
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    let url = crate::sentence::RUNTIME_URL;

    let llm_dir = crate::sentence::llm_dir();
    std::fs::create_dir_all(&llm_dir).map_err(|e| format!("could not create llm dir: {e}"))?;
    let tgz_path = llm_dir.join("runtime.tgz");
    let part_path = llm_dir.join("runtime.tgz.part");

    // Helper: emit a progress event (best-effort). Fixed `id: "runtime"`.
    let emit_progress = |received: u64, total: u64, done: bool, error: Option<String>| {
        let _ = app.emit(
            EVT_MODEL_DOWNLOAD_PROGRESS,
            ModelDownloadProgress {
                id: "runtime".to_string(),
                received,
                total,
                done,
                error,
            },
        );
    };

    // Helper: clean up the partial file, emit an error, and return Err.
    let fail = |msg: String| -> Result<(), String> {
        let _ = std::fs::remove_file(&part_path);
        emit_progress(0, 0, false, Some(msg.clone()));
        Err(msg)
    };

    // reqwest is built with `rustls-no-provider`, so a process-default rustls
    // CryptoProvider must be installed before the first TLS handshake. Install
    // ring (idempotent — Err means another component already set one).
    let _ = rustls::crypto::ring::default_provider().install_default();

    let resp = match reqwest::Client::new().get(url).send().await {
        Ok(r) => r,
        Err(e) => return fail(format!("request failed: {e}")),
    };
    if !resp.status().is_success() {
        return fail(format!("download failed: HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);

    let mut file = match std::fs::File::create(&part_path) {
        Ok(f) => f,
        Err(e) => return fail(format!("could not create file: {e}")),
    };

    let mut received: u64 = 0;
    // Throttle progress to at most ~1MB or ~1% of total, whichever is smaller.
    let step = if total > 0 {
        (total / 100).clamp(1, 1_048_576)
    } else {
        1_048_576
    };
    let mut next_emit: u64 = 0;
    emit_progress(0, total, false, None);

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let bytes = match chunk {
            Ok(b) => b,
            Err(e) => return fail(format!("stream error: {e}")),
        };
        if let Err(e) = file.write_all(&bytes) {
            return fail(format!("write error: {e}"));
        }
        received += bytes.len() as u64;
        if received >= next_emit {
            emit_progress(received, total, false, None);
            next_emit = received + step;
        }
    }

    // Flush + fsync before the atomic rename so a crash can't leave a torn final.
    if let Err(e) = file.flush().and_then(|_| file.sync_all()) {
        return fail(format!("fsync error: {e}"));
    }
    drop(file);
    if let Err(e) = std::fs::rename(&part_path, &tgz_path) {
        return fail(format!("rename error: {e}"));
    }

    // Extract the FLAT-file tarball directly into `llm_dir()` via system `tar`
    // (no new crate). The archive cleanup helper differs from `fail` (it removes
    // the renamed .tgz, not the .part), so clean inline on extract failure.
    let extract_failed = |msg: String| -> Result<(), String> {
        let _ = std::fs::remove_file(&tgz_path);
        emit_progress(0, 0, false, Some(msg.clone()));
        Err(msg)
    };
    match std::process::Command::new("tar")
        .arg("xzf")
        .arg(&tgz_path)
        .arg("-C")
        .arg(&llm_dir)
        .status()
    {
        Ok(s) if s.success() => {}
        Ok(s) => return extract_failed(format!("extract failed: tar exited with {s}")),
        Err(e) => return extract_failed(format!("extract failed: could not run tar: {e}")),
    }

    // Ensure the binary is executable (tar should preserve mode, but be sure).
    #[cfg(unix)]
    {
        let _ = std::fs::set_permissions(
            crate::sentence::llama_bin(),
            std::fs::Permissions::from_mode(0o755),
        );
    }

    // Best-effort: clear the macOS quarantine xattr so Gatekeeper doesn't block
    // the freshly-downloaded binary/dylibs. Ignore errors.
    let _ = std::process::Command::new("xattr")
        .args(["-dr", "com.apple.quarantine"])
        .arg(&llm_dir)
        .status();

    // Delete the archive after a successful extract (saves ~18MB).
    let _ = std::fs::remove_file(&tgz_path);

    // Verify the extract actually produced the binary.
    if !crate::sentence::runtime_installed() {
        return extract_failed(
            "runtime archive did not contain the expected binary".to_string(),
        );
    }

    emit_progress(received, total, true, None);
    Ok(())
}

/// Generate a natural practice sentence by shelling out to the staged local
/// `llama-completion` binary. We TRUST the model to write correct, natural
/// English and bias it toward the user's chords via a seed-word prompt (up to
/// ~12 random library words). No grammar constraint — the model is free, then
/// each word is GRADED after the fact by lemma recognition.
///
/// Returns the tokenized sentence: each token carries its display text and an
/// `is_glue` flag. `is_glue == false` (graded) when the word is a known chord —
/// a library word, an inflection whose base form is a library chord, or a glue
/// word. `is_glue == true` (typed but NOT graded) for genuinely-novel words —
/// the "expand your chord library" cues.
/// `Err("Sentence model not set up")` when the binary or model is missing.
#[tauri::command]
pub async fn generate_sentence(
    state: State<'_, AppState>,
    size: String,
) -> Result<Vec<SentenceToken>, String> {
    // The llama binary must be staged, and a model (managed or legacy) must be
    // resolvable. Resolve the absolute model path up front so it's threaded into
    // the blocking task (which can't touch `State`).
    if !crate::sentence::llama_bin().exists() {
        return Err("Sentence model not set up".to_string());
    }
    let (model_path, english_variant) = {
        let settings = state.settings.lock();
        (
            crate::sentence::active_model_path(&settings),
            settings.english_variant.clone(),
        )
    };
    let model_path = match model_path {
        Some(p) => p,
        None => return Err("Sentence model not set up".to_string()),
    };
    if state.storage.lock().is_none() {
        return Err("database not unlocked".to_string());
    }

    let flow_size = crate::sentence::FlowSize::parse(&size);

    let tokens = tauri::async_runtime::spawn_blocking(move || -> Result<Vec<SentenceToken>, String> {
        // Read the practiceable single-word library phrases on a fresh
        // connection (WAL read; doesn't touch the shared write handle). We
        // recognize inflections via lemma at grading time, so no inflection
        // generation here — just the raw library set.
        let mut library_words: Vec<String> = match Storage::open() {
            Ok(conn) => Storage::from_connection(conn).practiceable_words(),
            Err(e) => return Err(format!("could not read chord library: {e}")),
        };
        // Sentence-vocab filter: drop single-letter chords (b/c/d/…) which make
        // generated text read like noise; keep real words + "a"/"i".
        library_words.retain(|w| w.chars().count() >= 2 || w == "a" || w == "i");
        let library_set: std::collections::HashSet<String> =
            library_words.iter().cloned().collect();
        if library_set.is_empty() {
            return Err("no practiceable chords in the library yet".to_string());
        }
        let glue = crate::sentence::glue_set();

        // Pick up to ~12 RANDOM library words as seeds to bias the sentence
        // toward the user's chords; also vary the RNG seed for variety.
        let rand = quick_rand();
        let mut pool: Vec<&String> = library_set.iter().collect();
        // Fisher–Yates-ish shuffle driven by the cheap RNG, then take the first 12.
        let mut r = rand;
        for i in (1..pool.len()).rev() {
            r = r.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let j = (r >> 33) as usize % (i + 1);
            pool.swap(i, j);
        }
        let seeds: Vec<&str> = pool.iter().take(12).map(|s| s.as_str()).collect();
        let seed_list = seeds.join(", ");
        let llama_seed = (rand >> 1) as i64; // non-negative
        let len_word = flow_size.length_word();
        // Bias spelling to the user's variant (their chords use this spelling).
        // Prompt-only: grading does NOT US<->UK lemma-normalize (out of scope).
        let spelling = if english_variant == "uk" {
            " Use British English spelling."
        } else {
            " Use American English spelling."
        };
        let prompt = format!(
            "Write a natural {len_word} sentence using some of these words: {seed_list}.{spelling} "
        );

        // Token budget scales with the size's upper word bound (~4 tokens/word)
        // so an L sentence isn't cut short by the `-n` cap.
        let token_budget = flow_size.max_words() * 4;

        // Run the staged binary with CURRENT DIR = llm_dir so the dylibs resolve.
        // The model is passed as an ABSOLUTE path (it may live under models/ or be
        // the legacy llm_dir()/model.gguf), so it resolves regardless of cwd.
        let llm_dir = crate::sentence::llm_dir();
        let output = std::process::Command::new(crate::sentence::llama_bin())
            .current_dir(&llm_dir)
            .arg("-m")
            .arg(&model_path)
            .arg("-n")
            .arg(token_budget.to_string())
            .arg("--temp")
            .arg("0.7")
            .arg("--top-p")
            .arg("0.9")
            .arg("--repeat-penalty")
            .arg("1.3")
            .arg("--repeat-last-n")
            .arg("64")
            // Force raw completion. Instruct models (SmolLM2, Qwen) carry a chat
            // template, which llama-completion auto-applies — wrapping the output
            // in user/assistant turns ("Assistant …" + "> EOF by user"). We want
            // a plain sentence, not a chat reply.
            .arg("--no-conversation")
            .arg("--seed")
            .arg(llama_seed.to_string())
            .arg("-p")
            .arg(&prompt)
            .output()
            .map_err(|e| format!("failed to run sentence model: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("sentence model exited with error: {stderr}"));
        }

        // Parse stdout: it echoes the prompt then the completion, ending with
        // `[end of text]`. Strip the prompt prefix and the marker, then trim.
        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        let after_prompt = match raw.find(&prompt) {
            Some(i) => raw[i + prompt.len()..].to_string(),
            None => raw,
        };
        // Instruct models (e.g. Gemma) emit markdown + typographic punctuation.
        // Strip markdown emphasis markers (*, _, `, #, ~), and fold smart quotes /
        // dashes / ellipsis to ASCII so the displayed token matches what the
        // user's chords produce (ASCII " ' -) instead of curly “ ” ‘ ’.
        let sentence: String = after_prompt
            .replace("[end of text]", "")
            .replace(['\u{201C}', '\u{201D}'], "\"")
            .replace(['\u{2018}', '\u{2019}'], "'")
            .replace(['\u{2013}', '\u{2014}'], "-")
            .replace('\u{2026}', "...")
            .chars()
            .filter(|c| !matches!(c, '*' | '_' | '`' | '#' | '~'))
            .collect::<String>()
            .trim()
            .to_string();

        // Tokenize on whitespace; grade each word by lemma recognition. A word is
        // glue (NOT graded) iff it's NOT a known chord — i.e. not a library word,
        // not an inflection of one, and not a glue word.
        let tokens: Vec<SentenceToken> = sentence
            .split_whitespace()
            .map(|tok| {
                // Strip surrounding punctuation for the recognition lookup, but
                // keep the original token text (case + punctuation) for display.
                let key: String = tok
                    .trim_matches(|c: char| !c.is_alphabetic())
                    .to_lowercase();
                let is_glue = !crate::sentence::is_known_chord(&key, &library_set, &glue);
                // Inflection hint: when the token isn't itself a chord (or glue)
                // but a lemma of it IS in the library ("changing" → "change"),
                // expose that base so the UI can tell the user which base chord to
                // use. Empty for direct chords, glue, and novel words.
                let base_word = if library_set.contains(&key) || glue.contains(key.as_str()) {
                    String::new()
                } else {
                    crate::storage::lemma_bases(&key)
                        .into_iter()
                        .find(|b| library_set.contains(b))
                        .unwrap_or_default()
                };
                SentenceToken {
                    text: tok.to_string(),
                    is_glue,
                    base_word,
                }
            })
            .filter(|t| !t.text.is_empty())
            .collect();

        Ok(tokens)
    })
    .await
    .map_err(|e| format!("sentence generation task failed: {e}"))?;

    tokens
}
