// Global keyboard shortcut system. Each user-configurable accelerator string
// (stored in `Settings`) maps to a logical `ShortcutAction`. The plugin's single
// global handler (installed in `lib.rs`) dispatches a fired shortcut to its
// action via the `Shortcut -> action` map published in `AppState`.
//
// Bindings are re-registered from scratch on startup and whenever the user edits
// them in Settings (`set_settings` -> `reregister`). Registration is OS-level, so
// the shortcuts fire regardless of which app is focused.

use std::str::FromStr;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::AppState;

/// Default accelerators (Tauri/Electron accelerator syntax). `CmdOrCtrl` resolves
/// to ⌘ on macOS and Ctrl elsewhere.
pub const DEFAULT_RELOAD_CHORDS: &str = "CmdOrCtrl+Shift+R";
pub const DEFAULT_FORCE_COACHING: &str = "CmdOrCtrl+Shift+C";

/// A logical action a global shortcut can trigger.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShortcutAction {
    /// Background chordmap refresh (the long-standing ⌘⇧R action).
    ReloadChords,
    /// Force-show the coaching overlay using the last computed hint.
    ForceShowCoaching,
}

/// Parse an accelerator string into a `Shortcut`. Returns `None` for empty or
/// unparseable input (the caller logs + skips it rather than crashing).
pub fn parse(accel: &str) -> Option<Shortcut> {
    let trimmed = accel.trim();
    if trimmed.is_empty() {
        return None;
    }
    Shortcut::from_str(trimmed).ok()
}

/// The (accelerator, action) pairs derived from the current settings.
fn bindings(app: &AppHandle) -> Vec<(String, ShortcutAction)> {
    let state = app.state::<AppState>();
    let s = state.settings.lock();
    vec![
        (
            s.shortcut_reload_chords.clone(),
            ShortcutAction::ReloadChords,
        ),
        (
            s.shortcut_force_coaching.clone(),
            ShortcutAction::ForceShowCoaching,
        ),
    ]
}

/// Unregister all global shortcuts, then register the ones in the current
/// settings and republish the `Shortcut -> action` dispatch map. Invalid or
/// duplicate accelerators are logged and skipped so one bad binding can't take
/// down the others. Safe to call repeatedly (startup + every settings save).
pub fn reregister(app: &AppHandle) {
    let gs = app.global_shortcut();
    if let Err(e) = gs.unregister_all() {
        crate::logging::log_line(&format!("[SHORTCUT] unregister_all failed: {e}"));
    }

    let state = app.state::<AppState>();
    let mut map = state.shortcut_actions.lock();
    map.clear();

    for (accel, action) in bindings(app) {
        let Some(shortcut) = parse(&accel) else {
            crate::logging::log_line(&format!("[SHORTCUT] skip invalid accelerator '{accel}'"));
            continue;
        };
        // A second binding mapped to the same chord would silently shadow the
        // first; skip the collision and keep the earlier action.
        if map.contains_key(&shortcut) {
            crate::logging::log_line(&format!("[SHORTCUT] '{accel}' collides — skipped"));
            continue;
        }
        match gs.register(shortcut) {
            Ok(()) => {
                map.insert(shortcut, action);
                crate::logging::log_line(&format!("[SHORTCUT] registered '{accel}' -> {action:?}"));
            }
            Err(e) => {
                crate::logging::log_line(&format!("[SHORTCUT] register '{accel}' failed: {e}"));
            }
        }
    }
}

/// Dispatch a fired shortcut to its action. Wired as the plugin's global handler
/// in `lib.rs`. Only acts on the key-DOWN edge.
pub fn handle(app: &AppHandle, shortcut: &Shortcut, state: ShortcutState) {
    if state != ShortcutState::Pressed {
        return;
    }
    let action = {
        let st = app.state::<AppState>();
        let map = st.shortcut_actions.lock();
        map.get(shortcut).copied()
    };
    match action {
        Some(ShortcutAction::ReloadChords) => {
            crate::logging::log_line("[SYNC] global hotkey -> reload chords");
            crate::commands::run_background_refresh(app.clone());
        }
        Some(ShortcutAction::ForceShowCoaching) => {
            crate::logging::log_line("[COACH] global hotkey -> force show coaching");
            crate::commands::force_show_coaching(app.clone());
        }
        None => {}
    }
}
