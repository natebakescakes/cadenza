// Coaching overlay NSPanel — Phase 4 (Option A, `tauri-nspanel` v2.1 branch).
//
// Builds a transparent, floating, non-activating, click-through NSPanel that
// loads the dedicated `overlay.html` Vite entry (created by the frontend). The
// panel starts HIDDEN. It is positioned + shown in Rust from the
// `coaching_position` event (logical NS coords resolved by the AX locator).
//
// API surface used from `tauri-nspanel` v2.1 (verified against the pulled
// crate source at ~/.cargo/git/checkouts/tauri-nspanel-*):
//   - `tauri_nspanel::init()` -> plugin registered in the builder chain. It only
//     manages a `WebviewPanelManager` state; it declares NO permissions/commands
//     (so the capability needs only core window perms, no `nspanel:` perms).
//   - `tauri_panel! { panel!(Name { config: { ... } }) }` defines a custom
//     NSPanel subclass (the macro emits `use` statements, so it lives in this
//     dedicated module). We set `can_become_key_window:false`,
//     `can_become_main_window:false`, `is_floating_panel:true`.
//   - `PanelBuilder::<_, CoachingOverlayPanel>::new(&app_handle, LABEL)` fluent
//     builder: `.url`, `.size`, `.level(PanelLevel::Floating)`, `.transparent`,
//     `.has_shadow(false)`, `.hides_on_deactivate(false)`,
//     `.becomes_key_only_if_needed(true)`, `.ignores_mouse_events(true)`,
//     `.no_activate(true)`, `.style_mask(StyleMask::empty().nonactivating_panel())`,
//     `.collection_behavior(...)`, `.with_window(|w| w.decorations(false)...)`
//     `.build() -> tauri::Result<Arc<dyn Panel>>`.
//   - Runtime: `app.get_webview_panel(LABEL)` -> `Arc<dyn Panel>`; `.hide()` /
//     `.order_front_regardless()` (show WITHOUT activating — NOT
//     `show_and_make_key`, which steals focus). Positioning/sizing go through the
//     underlying `WebviewWindow` (same label) via `set_position` / `set_size`
//     since the `Panel` trait exposes no `set_frame`.

use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl};
use tauri_nspanel::{
    tauri_panel, CollectionBehavior, ManagerExt, PanelBuilder, PanelLevel, StyleMask,
};

use crate::logging::log_line;
use crate::types::ScreenRect;

/// Window label for the overlay panel. Scoped in `capabilities/overlay.json`.
pub const OVERLAY_LABEL: &str = "overlay";

// Fixed logical panel size. Sized generously to fit the richer "suggested combo
// + alternatives + conflicts" layout; the panel is transparent and click-through,
// so a minimal single-chord reminder simply leaves the unused area invisible. The
// webview content anchors top-left (just under the caret).
const OVERLAY_W: f64 = 320.0;
const OVERLAY_H: f64 = 240.0;

// Define the non-activating, floating NSPanel subclass. The `tauri_panel!`
// macro emits `use` statements, hence its own module scope.
tauri_panel! {
    panel!(CoachingOverlayPanel {
        config: {
            can_become_key_window: false,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })
}

/// Build the overlay NSPanel (hidden). Call once from `.setup()` on the main
/// thread. Best-effort: logs and returns on any failure so startup never aborts.
pub fn build_overlay_panel(app_handle: &AppHandle) {
    // Idempotent: if it already exists, do nothing.
    if app_handle.get_webview_panel(OVERLAY_LABEL).is_ok() {
        return;
    }

    let res = PanelBuilder::<_, CoachingOverlayPanel>::new(app_handle, OVERLAY_LABEL)
        .url(WebviewUrl::App("overlay.html".into()))
        .size(tauri::Size::Logical(LogicalSize {
            width: OVERLAY_W,
            height: OVERLAY_H,
        }))
        // AC11: always-on-top floating, transparent, non-activating, click-through.
        .level(PanelLevel::Floating)
        .transparent(true)
        .has_shadow(false)
        .hides_on_deactivate(false)
        .becomes_key_only_if_needed(true)
        .ignores_mouse_events(true)
        // Show across all Spaces without stealing focus or joining Cmd+Tab cycle.
        .collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .stationary()
                .ignores_cycle(),
        )
        .style_mask(StyleMask::empty().nonactivating_panel())
        // Prevent focus theft during window creation.
        .no_activate(true)
        .with_window(|w| {
            w.decorations(false)
                .transparent(true)
                .background_color(tauri::window::Color(0, 0, 0, 0))
                .skip_taskbar(true)
                .always_on_top(true)
        })
        .build();

    match res {
        Ok(panel) => {
            // Built shown by default in some paths; ensure hidden at startup.
            panel.hide();
            log_line("coaching: overlay NSPanel created (hidden)");
        }
        Err(e) => {
            log_line(&format!("coaching: failed to build overlay panel: {e:?}"));
        }
    }
}

// Gap (logical px) between the overlay content and the caret.
const OVERLAY_GAP: f64 = 6.0;

/// Position the overlay at `rect` (logical NS coords) and show it WITHOUT
/// activating. Must run on the main thread. Best-effort; logs on failure.
///
/// Positioning policy: place the panel ABOVE the caret (where the user is
/// looking). The webview content is bottom-anchored within the panel, so the
/// panel's BOTTOM edge is set just above the caret/field top (`rect.y`) and the
/// content hugs that edge — the transparent remainder extends upward and is
/// invisible. Tauri logical window positions are top-left origin (matching the
/// locator's emitted coords), so no flip happens here.
///
/// When `centered` (no real caret was found, e.g. Ghostty), `rect` is the screen
/// centre point: the panel is centred HORIZONTALLY on it instead of left-anchored
/// at a caret, so the overlay lands predictably in the middle of the screen.
pub fn position_and_show(app_handle: &AppHandle, rect: &ScreenRect, centered: bool) {
    let panel = match app_handle.get_webview_panel(OVERLAY_LABEL) {
        Ok(p) => p,
        Err(_) => {
            log_line("coaching: position_and_show — overlay panel not found");
            return;
        }
    };

    // Position via the underlying WebviewWindow (same label); the Panel trait
    // has no set_frame. set_position/set_size accept logical units.
    if let Some(window) = app_handle.get_webview_window(OVERLAY_LABEL) {
        // Caret anchor: left-aligned at the caret. Centre fallback: centre the
        // panel horizontally on the screen-centre point.
        let x = if centered {
            rect.x - OVERLAY_W / 2.0
        } else {
            rect.x
        };
        // Panel top so its BOTTOM edge sits `OVERLAY_GAP` above the anchor point.
        // Content is bottom-anchored in the webview, so it appears just above
        // the anchor. Clamp at 0 so it never goes off the top of the screen.
        let y = (rect.y - OVERLAY_H - OVERLAY_GAP).max(0.0);
        let _ = window.set_size(LogicalSize {
            width: OVERLAY_W,
            height: OVERLAY_H,
        });
        let _ = window.set_position(LogicalPosition { x, y });
    }

    // Show WITHOUT activating (no makeKey) — preserves the focused app.
    panel.order_front_regardless();
}

/// Force the app back to a regular foreground app (Dock icon + menu bar).
///
/// `tauri-nspanel`'s `no_activate` builder toggles the NSApplication activation
/// policy to `Prohibited` during panel creation and restores whatever policy it
/// captured beforehand (builder.rs:817-928). Under `tauri dev` the binary is not
/// a real `.app` bundle, so at `.setup()` time the policy has not settled to
/// `Regular` yet — nspanel then restores the non-regular value, leaving the app
/// as an accessory (no Dock icon; shows the parent terminal) and suppressing the
/// Accessibility prompt. Call this AFTER building the panel to pin `Regular`.
/// Must run on the main thread.
pub fn ensure_regular_activation_policy() {
    if let Some(mtm) = objc2_foundation::MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    }
}

/// Hide the overlay panel. Must run on the main thread. Best-effort.
pub fn hide_overlay(app_handle: &AppHandle) {
    if let Ok(panel) = app_handle.get_webview_panel(OVERLAY_LABEL) {
        panel.hide();
    }
}

/// Hide the overlay whenever the user switches to a different application.
///
/// The panel is non-activating + `hides_on_deactivate(false)` + joins all Spaces,
/// so it survives app switches on its own — and in persist mode nothing else
/// dismisses it without a keystroke. We observe `NSWorkspace`'s
/// "did activate application" notification (posted on the main thread) and, on
/// any app activation, clear the visibility flag, tell the overlay to dismiss,
/// and hide the panel. Call once from `.setup()` on the main thread; the
/// observer token is leaked intentionally (it lives for the whole app).
pub fn install_focus_change_observer(
    app_handle: AppHandle,
    visible: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use block2::RcBlock;
    use objc2_app_kit::{NSWorkspace, NSWorkspaceDidActivateApplicationNotification};

    let Some(_mtm) = objc2_foundation::MainThreadMarker::new() else {
        log_line("coaching: focus observer skipped — not on main thread");
        return;
    };

    let block = RcBlock::new(move |_notif: core::ptr::NonNull<objc2_foundation::NSNotification>| {
        // Runs on the main thread (workspace notifications post there), so AppKit
        // calls are safe. Only act if the overlay is currently up.
        if visible.swap(false, std::sync::atomic::Ordering::Relaxed) {
            let _ = app_handle.emit(crate::EVT_COACHING_DISMISS, ());
            hide_overlay(&app_handle);
        }
    });

    unsafe {
        let workspace = NSWorkspace::sharedWorkspace();
        let center = workspace.notificationCenter();
        let token = center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceDidActivateApplicationNotification),
            None,
            None,
            &block,
        );
        // Keep the observer alive for the app's lifetime.
        std::mem::forget(token);
    }
    log_line("coaching: focus-change observer installed");
}
