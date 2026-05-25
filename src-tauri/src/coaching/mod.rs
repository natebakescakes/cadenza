// macOS Accessibility caret locator for the chord coaching overlay.
//
// Phase 2 implements the real staged tiered locator in `caret.rs`:
// Chromium TextMarker → native parameterized range → mirror frame → None.
// The whole module is macOS-gated in `lib.rs`, so no non-macOS fallback is
// needed here (the engine only calls `locate_caret()` under `cfg(macos)`).
//
// The stable interface stays `pub fn locate_caret() -> Option<ScreenRect>` so
// the engine's `exec_async`-to-main dispatch seam is unchanged.

mod caret;
mod overlay;
mod permission;

pub use caret::locate_caret;
pub use overlay::{
    build_overlay_panel, ensure_regular_activation_policy, hide_overlay,
    install_focus_change_observer, position_and_show,
};
pub use permission::prompt_accessibility_trust;
