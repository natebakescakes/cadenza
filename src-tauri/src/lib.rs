mod commands;
mod engine;
mod keylogger;
mod logging;
#[cfg(target_os = "macos")]
mod macos_layout;
mod serial;
mod storage;
mod types;

use std::collections::HashSet;
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use parking_lot::{Mutex, RwLock};
use tauri::{AppHandle, Manager};

use crate::engine::{DetectorHandle, Engine};
use crate::keylogger::KeyLogger;
use crate::serial::Device;
use crate::storage::Storage;
use crate::types::{DeviceInfo, DeviceSettings, KeyEvent, LoggingState, Settings};

// --- Events the backend emits (documented; not emitted yet) ----------------
pub const EVT_KEYSTROKE: &str = "keystroke";
pub const EVT_WPM: &str = "wpm";
pub const EVT_WORD_LOGGED: &str = "word_logged";
pub const EVT_CHORD_LOGGED: &str = "chord_logged";
pub const EVT_LOGGING_STATE: &str = "logging_state";
pub const EVT_DEVICE_CHANGED: &str = "device_changed";

/// Shared application state managed by Tauri.
pub struct AppState {
    /// Open storage handle (None until the DB is initialized/unlocked).
    pub storage: Mutex<Option<Storage>>,
    /// Combined logging + db-unlocked flags surfaced to the UI.
    pub logging_state: Mutex<LoggingState>,
    /// User-tunable detection settings, shared with the live detector thread.
    pub settings: Arc<Mutex<Settings>>,
    /// Currently connected device info, if any (surfaced to the UI).
    pub device: Mutex<Option<DeviceInfo>>,
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
}

impl Default for AppState {
    fn default() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded::<KeyEvent>();
        Self {
            storage: Mutex::new(None),
            logging_state: Mutex::new(LoggingState::default()),
            settings: Arc::new(Mutex::new(Settings::default())),
            device: Mutex::new(None),
            device_conn: Mutex::new(None),
            keylogger: Mutex::new(KeyLogger::default()),
            engine: Mutex::new(Engine::default()),
            key_tx: tx,
            key_rx: Mutex::new(Some(rx)),
            detector: Mutex::new(None),
            app_handle: Mutex::new(None),
            chord_phrases: Arc::new(RwLock::new(HashSet::new())),
            device_settings: Mutex::new(None),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Durable crash logging: panic hook writes to cadenza.log + stderr.
    crate::logging::install_panic_hook();
    crate::logging::log_line("Cadenza starting");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
            commands::list_banlist,
            commands::ban_word,
            commands::unban_word,
            commands::hide_word,
            commands::unhide_word,
            commands::list_hidden,
            commands::get_device_settings,
            commands::resync_device_thresholds,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
