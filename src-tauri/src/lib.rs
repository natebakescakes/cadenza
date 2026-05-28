#[cfg(target_os = "macos")]
mod coaching;
mod commands;
mod engine;
mod keylogger;
mod logging;
#[cfg(target_os = "macos")]
mod macos_layout;
mod sentence;
mod serial;
mod storage;
mod types;

use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "macos")]
use std::sync::atomic::AtomicI64;
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use parking_lot::{Mutex, RwLock};
#[cfg(target_os = "macos")]
use tauri::Listener;
use tauri::{AppHandle, Manager};

use crate::engine::{DetectorHandle, Engine};
use crate::keylogger::KeyLogger;
use crate::serial::Device;
use crate::storage::Storage;
use crate::types::{DeviceInfo, DeviceSettings, KeyEvent, LoggingState, Settings};

// --- Events the backend emits (documented; not emitted yet) ----------------
pub const EVT_KEYSTROKE: &str = "keystroke";
pub const EVT_COACHING_HINT: &str = "coaching_hint";
/// Empty-payload signal telling the overlay webview to dismiss on the next key.
/// Replaces the privacy-sensitive `EVT_KEYSTROKE` emit while the overlay is up —
/// the overlay only needs "a key happened", never the typed character.
pub const EVT_COACHING_DISMISS: &str = "coaching_dismiss";
pub const EVT_COACHING_POSITION: &str = "coaching_position";
pub const EVT_WPM: &str = "wpm";
pub const EVT_WORD_LOGGED: &str = "word_logged";
pub const EVT_CHORD_LOGGED: &str = "chord_logged";
pub const EVT_LOGGING_STATE: &str = "logging_state";
pub const EVT_DEVICE_CHANGED: &str = "device_changed";
/// Generic overlay surface events (kind-tagged) for the shared overlay NSPanel.
/// Payload shapes: show/update = {kind, payload}, hide = {kind}. The sync surface
/// uses kind="sync" with payload {state, count?, message?}. The frontend owns
/// auto-hide + panel teardown; the backend only emits show/update.
pub const EVT_OVERLAY_SHOW: &str = "overlay:show";
pub const EVT_OVERLAY_UPDATE: &str = "overlay:update";
pub const EVT_OVERLAY_HIDE: &str = "overlay:hide";
/// Emitted on each chord fire WHILE practice mode is active (instead of any
/// ambient stat write/emit). Payload: `{ phrase: String, fire_ms: f64, correct: bool }`.
pub const EVT_PRACTICE_CHORD: &str = "practice_chord";
/// Throttled Sentence-mode model download progress. Payload (snake_case):
/// `{ id, received, total, done, error? }`.
pub const EVT_MODEL_DOWNLOAD_PROGRESS: &str = "model_download_progress";

/// Shared application state managed by Tauri.
pub struct AppState {
    /// Open storage handle (None until the DB is initialized/unlocked).
    pub storage: Mutex<Option<Storage>>,
    /// Combined logging + db-unlocked flags surfaced to the UI.
    pub logging_state: Mutex<LoggingState>,
    /// User-tunable detection settings, shared with the live detector thread.
    pub settings: Arc<Mutex<Settings>>,
    /// Currently connected device info, if any (surfaced to the UI). Shared with
    /// the detector thread so it can resolve `device_id` LIVE for coaching.
    pub device: Arc<Mutex<Option<DeviceInfo>>>,
    /// Open serial connection to the device, if any (used to read the chord map).
    pub device_conn: Mutex<Option<Device>>,
    /// Global keyboard hook controller.
    pub keylogger: Mutex<KeyLogger>,
    /// Detection engine state (settings mirror; live loop runs on its own thread).
    pub engine: Mutex<Engine>,
    /// Channel from the keylogger thread to the detector thread.
    pub key_tx: Sender<KeyEvent>,
    pub key_rx: Mutex<Option<Receiver<KeyEvent>>>,
    /// Handle to the running detector thread, if logging is active.
    pub detector: Mutex<Option<DetectorHandle>>,
    /// AppHandle captured during `.setup()` for event emission.
    pub app_handle: Mutex<Option<AppHandle>>,
    /// Normalized (lowercase+trim) set of device chord phrases for fast
    /// arpeggio/compound-chord lookup in the detector thread.
    pub chord_phrases: Arc<RwLock<HashSet<String>>>,
    /// Last-read raw device settings (None until first connect or resync).
    pub device_settings: Mutex<Option<DeviceSettings>>,
    /// True while a coaching overlay is visible. Gates `EVT_KEYSTROKE` emission
    /// in the detector's `process()` so it doesn't flood IPC in steady state.
    pub coaching_overlay_visible: Arc<AtomicBool>,
    /// Process-global monotonic coaching hint id source. Lives in AppState (not
    /// the Detector) so the id space keeps climbing across detector respawns
    /// (stop/start logging). The `coaching_position` listener in `.setup()`
    /// keeps a monotonic high-water mark and drops any lower id; a per-Detector
    /// counter would reset to 0 on respawn and get its positions silently
    /// dropped until it climbed back past the watermark — the overlay would
    /// fail to appear, then "recover" on its own.
    pub coaching_hint_seq: Arc<AtomicI64>,
    /// Chordmap generation counter, bumped on every `refresh_chordmap`. Shared
    /// with the detector thread, which compares it against the generation its
    /// cached coaching chord maps were built at and rebuilds when it climbs —
    /// the live invalidation path for the per-session map cache.
    pub chordmap_gen: Arc<AtomicI64>,
    /// True while a practice (spaced-repetition drill) session is active. When
    /// set, the detector FULLY SUPPRESSES all ambient stat writes + emits and
    /// instead emits `practice_chord` on each chord fire. Shared with the
    /// detector thread so the gate is live without a respawn.
    pub practice_active: Arc<AtomicBool>,
    /// The phrase the user is currently being asked to drill (case-insensitive
    /// match drives the `correct` flag on `practice_chord`). `None` when idle.
    pub practice_target: Arc<Mutex<Option<String>>>,
    /// Cached Sentence-mode GBNF trie BODY, keyed on `chordmap_gen`. Tuple is
    /// `(gen, body, library_words, glue_set)`: the generation it was built at,
    /// the size-INDEPENDENT trie body (`word ::= …` + node rules — the expensive
    /// part), the lowercased practiceable library words (for grading: a token is
    /// glue iff NOT in this set), and the fixed glue set. The size-specific
    /// `root` line is prepended per request (see `sentence::assemble_grammar`),
    /// so changing S/M/L reuses this body. Rebuilt when `chordmap_gen` climbs so
    /// the chord library can change live.
    pub sentence_grammar:
        Arc<Mutex<Option<(i64, String, HashSet<String>, HashSet<String>)>>>,
}

impl Default for AppState {
    fn default() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded::<KeyEvent>();
        Self {
            storage: Mutex::new(None),
            logging_state: Mutex::new(LoggingState::default()),
            settings: Arc::new(Mutex::new(Settings::default())),
            device: Arc::new(Mutex::new(None)),
            device_conn: Mutex::new(None),
            keylogger: Mutex::new(KeyLogger::default()),
            engine: Mutex::new(Engine::default()),
            key_tx: tx,
            key_rx: Mutex::new(Some(rx)),
            detector: Mutex::new(None),
            app_handle: Mutex::new(None),
            chord_phrases: Arc::new(RwLock::new(HashSet::new())),
            device_settings: Mutex::new(None),
            coaching_overlay_visible: Arc::new(AtomicBool::new(false)),
            coaching_hint_seq: Arc::new(AtomicI64::new(0)),
            chordmap_gen: Arc::new(AtomicI64::new(0)),
            practice_active: Arc::new(AtomicBool::new(false)),
            practice_target: Arc::new(Mutex::new(None)),
            sentence_grammar: Arc::new(Mutex::new(None)),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Durable crash logging: panic hook writes to cadenza.log + stderr.
    crate::logging::install_panic_hook();
    crate::logging::log_line("Cadenza starting");

    // Global hotkey: CmdOrCtrl+Shift+R triggers the background chordmap refresh.
    // The handler fires on the plugin's thread; `run_background_refresh` dispatches
    // any AX/AppKit work to the main thread itself and spawns the heavy serial/DB
    // read on the blocking pool, so invoking it from this thread is safe.
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};
    let refresh_shortcut = Shortcut::new(
        Some(Modifiers::SUPER | Modifiers::SHIFT),
        Code::KeyR,
    );
    let handler_shortcut = refresh_shortcut;
    let global_shortcut = tauri_plugin_global_shortcut::Builder::new()
        .with_shortcut(refresh_shortcut)
        .expect("failed to register global refresh shortcut")
        .with_handler(move |app, shortcut, event| {
            if shortcut == &handler_shortcut && event.state() == ShortcutState::Pressed {
                crate::logging::log_line("[SYNC] global hotkey CmdOrCtrl+Shift+R");
                crate::commands::run_background_refresh(app.clone());
            }
        })
        .build();

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(global_shortcut);

    // macOS: register the NSPanel plugin for the coaching overlay window.
    #[cfg(target_os = "macos")]
    {
        builder = builder.plugin(tauri_nspanel::init());
    }

    builder
        .manage(AppState::default())
        .setup(|app| {
            let state = app.state::<AppState>();
            *state.app_handle.lock() = Some(app.handle().clone());

            // macOS: install the CGEventTap on the MAIN run loop NOW (we are on
            // the main thread inside `.setup()`). This is the only safe place to
            // touch TSM for layout capture. The tap is installed disabled;
            // `start_logging` enables it. On non-macOS this is a no-op (the
            // rdev thread is spawned lazily by `start_logging`).
            #[cfg(target_os = "macos")]
            {
                state.keylogger.lock().install_main_thread();

                // Phase 4: build the transparent floating overlay NSPanel (hidden).
                crate::coaching::build_overlay_panel(&app.handle().clone());

                // Hide the overlay when the user switches to a different app
                // (otherwise the non-activating panel lingers, esp. in persist mode).
                crate::coaching::install_focus_change_observer(
                    app.handle().clone(),
                    state.coaching_overlay_visible.clone(),
                );

                // tauri-nspanel's `no_activate` leaves the app's activation policy
                // at whatever it captured during panel creation; under `tauri dev`
                // that can be a non-regular value, which hides the Dock icon (the
                // app shows as the parent terminal) and suppresses the AX prompt.
                // Pin Regular AFTER building so the app is a normal foreground app.
                crate::coaching::ensure_regular_activation_policy();

                // Phase 3: request Accessibility trust (prompts once if needed).
                // Non-fatal — the caret locator early-returns None when untrusted,
                // so the overlay simply won't position until granted. Done AFTER
                // restoring Regular policy so the system prompt can surface.
                crate::coaching::prompt_accessibility_trust();

                // Phase 4.4: position + show the overlay on `coaching_position`.
                // `rect` is already logical NS coords (locator did any flip).
                // Track the latest hint id and ignore stale positions. The event
                // arrives on the main thread, so AppKit calls here are safe.
                let pos_handle = app.handle().clone();
                let latest_pos_id = Arc::new(AtomicI64::new(0));
                app.listen(EVT_COACHING_POSITION, move |event| {
                    if let Ok(pos) =
                        serde_json::from_str::<crate::types::CoachingPosition>(event.payload())
                    {
                        // Monotonic guard: ignore a position whose id is older than
                        // one we've already honored (a newer hint superseded it).
                        // The engine coalesces the caret locate to the latest hint,
                        // so only the current hint's position reaches here.
                        //
                        // We intentionally do NOT gate on `coaching_overlay_visible`:
                        // the backend auto-hide timer starts its clock at hint-emit,
                        // BEFORE this (async) position arrives, so a slightly slow
                        // locate would flip the flag false and the position would be
                        // dropped — leaving content rendered in a never-shown panel.
                        // The frontend owns content visibility; dismiss/fade/focus
                        // changes hide the (transparent) panel.
                        let prev = latest_pos_id.load(std::sync::atomic::Ordering::Relaxed);
                        if pos.id < prev {
                            return;
                        }
                        latest_pos_id.store(pos.id, std::sync::atomic::Ordering::Relaxed);
                        crate::coaching::position_and_show(&pos_handle, &pos.rect, pos.centered);
                    }
                });
            }

            crate::logging::log_line("Cadenza setup complete");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::is_db_initialized,
            commands::db_init,
            commands::db_unlock,
            commands::db_dev_unlock,
            commands::get_settings,
            commands::set_settings,
            commands::start_logging,
            commands::stop_logging,
            commands::logging_status,
            commands::list_words,
            commands::list_chords,
            commands::get_wpm_summary,
            commands::get_wpm_trend,
            commands::get_suggestions,
            commands::get_recent_blocks,
            commands::get_proficiency,
            commands::scan_devices,
            commands::connect_device,
            commands::current_device,
            commands::refresh_chordmap,
            commands::refresh_chords_bg,
            commands::debug_dump_chords,
            commands::show_overlay_at_caret,
            commands::list_banlist,
            commands::ban_word,
            commands::unban_word,
            commands::hide_word,
            commands::unhide_word,
            commands::list_hidden,
            commands::get_device_settings,
            commands::resync_device_thresholds,
            commands::hide_overlay,
            commands::set_overlay_interactive,
            commands::dismiss_overlay,
            commands::coach_log,
            commands::practice_begin,
            commands::practice_end,
            commands::practice_due_count,
            commands::practice_due_queue,
            commands::practice_all_queue,
            commands::practice_start_session,
            commands::practice_submit_result,
            commands::practice_card_stats,
            commands::practice_all_card_stats,
            commands::practice_overview,
            commands::practice_session_summary,
            commands::practice_complete_session,
            commands::generate_sentence,
            commands::list_models,
            commands::download_model,
            commands::set_active_model,
            commands::delete_model,
            commands::sentence_model_ready,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, _event| {
            // Pin a Regular foreground activation policy once the app has finished
            // launching. tauri-nspanel's `no_activate` panel build can leave the
            // policy at a non-regular value (no Dock icon / shows as the parent
            // terminal); doing this in `.setup()` runs too early to stick, so we
            // re-assert it on RunEvent::Ready (fires on the main thread post-launch).
            #[cfg(target_os = "macos")]
            if matches!(_event, tauri::RunEvent::Ready) {
                crate::coaching::ensure_regular_activation_policy();
            }
        });
}
