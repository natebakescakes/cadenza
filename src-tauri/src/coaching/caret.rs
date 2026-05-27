// macOS Accessibility (AX) caret locator — the real Phase 2 implementation.
//
// `locate_caret()` resolves a screen rect for the focused element's caret using
// a tiered strategy and returns it in Tauri **logical (NS, top-left origin)
// screen points** — the coordinate space `tauri-nspanel` set_frame expects.
//
// Tiers (tried in order, first non-empty rect wins):
//   1. Chromium/Electron  — `AXSelectedTextMarkerRange` →
//      `AXBoundsForTextMarkerRange` (custom AX attrs named by raw CFString).
//   2. Native AppKit      — insertion point from `kAXSelectedTextRangeAttribute`
//      → `kAXBoundsForRangeParameterizedAttribute` with `{loc, 1}` (a length-0
//      range returns kAXErrorNoValue, hence length 1).
//   3. Mirror             — focused element `AXPosition` + `AXSize` (the field
//      frame); the overlay floats above the field. Works uniformly everywhere.
//   else → None (caller emits no position; overlay hides).
//
// THREADING: every AX call here assumes it runs on the MAIN thread. The engine
// already dispatches `locate_caret()` inside `DispatchQueue::main().exec_async`,
// so we just do synchronous AX reads — no threading is added here.
//
// SAFETY/MEMORY: the AX *Copy* functions return +1-retained CFTypeRefs. We wrap
// each returned ref in core-foundation's `CFType::wrap_under_create_rule` (which
// CFReleases on drop) so nothing leaks. The whole body is wrapped in
// `catch_unwind` so no panic crosses the FFI/dispatch boundary.

use std::ffi::c_void;

use accessibility_sys::{
    kAXBoundsForRangeParameterizedAttribute, kAXFocusedUIElementAttribute, kAXPositionAttribute,
    kAXSelectedTextRangeAttribute, kAXSizeAttribute, kAXValueTypeCFRange, kAXValueTypeCGPoint,
    kAXValueTypeCGRect, kAXValueTypeCGSize, AXError, AXUIElementCopyAttributeValue,
    AXUIElementCopyParameterizedAttributeNames, AXUIElementCopyParameterizedAttributeValue,
    AXUIElementCreateSystemWide, AXUIElementRef, AXValueCreate, AXValueGetValue, AXValueRef,
    kAXErrorSuccess,
};
use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFRange, CFType, CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::geometry::{CGPoint, CGRect, CGSize};

use crate::logging::log_line;
use crate::types::ScreenRect;

// Chromium/Electron custom AX attribute names (not exported as constants in any
// crate — must be passed as raw CFStrings; this is why raw FFI is required).
const AX_SELECTED_TEXT_MARKER_RANGE: &str = "AXSelectedTextMarkerRange";
const AX_BOUNDS_FOR_TEXT_MARKER_RANGE: &str = "AXBoundsForTextMarkerRange";

/// A resolved overlay anchor. `centered` is true when no real caret/field could
/// be found and we fell back to the screen centre (so the caller centres the
/// panel horizontally instead of left-aligning at a caret).
pub struct CaretHit {
    pub rect: ScreenRect,
    pub centered: bool,
    /// `Some(app_name)` when the focused element is a Chromium-based browser
    /// (exposes `AXSelectedTextMarkerRange`) whose caret bounds came back
    /// degenerate — i.e. "Text Metrics" accessibility is OFF, so no real caret
    /// geometry is available. The overlay uses this to prompt the user to enable
    /// it (e.g. for Dia / Arc). `None` when a real caret was resolved.
    pub metrics_off_app: Option<String>,
}

/// Resolve an overlay anchor for the focused element, in Tauri logical NS coords.
///
/// Tries Chromium → native range → (small) mirror frame; if none yields a
/// usable caret/field, falls back to the active screen centre. Returns `None`
/// only when AX is untrusted/unavailable. Never panics (wrapped in catch_unwind).
pub fn locate_caret() -> Option<CaretHit> {
    std::panic::catch_unwind(locate_caret_inner).unwrap_or_else(|_| {
        log_line("coaching: locate_caret panicked (caught) — returning None");
        None
    })
}

fn locate_caret_inner() -> Option<CaretHit> {
    // Best-effort early-out: if the process is not AX-trusted, every AX read
    // below will fail anyway. We do NOT prompt here (that's Phase 3) — just
    // read the current trust state and bail quietly.
    if !unsafe { accessibility_sys::AXIsProcessTrusted() } {
        log_line("coaching: process not AX-trusted — caret locate skipped");
        return None;
    }

    let system_wide = unsafe { AXUIElementCreateSystemWide() };
    if system_wide.is_null() {
        return None;
    }
    // system_wide is +1 retained; wrap so it is released on drop.
    let _system_wide_guard = unsafe { wrap_cf(system_wide as CFTypeRef) }?;

    // Bound every AX read below. The default messaging timeout is ~6s, so a busy
    // or hung target app can block this (main-thread) call long enough to stall
    // the run loop and the CGEventTap. 0.25s is generous for a live caret query;
    // on timeout the call returns an error and we fall through to a lower tier.
    unsafe { accessibility_sys::AXUIElementSetMessagingTimeout(system_wide, 0.25) };

    // Focused UI element (the element receiving keystrokes). Even this can fail
    // for apps with no AX support (e.g. Ghostty) — fall through to centre.
    // Set when the focused element is a Chromium browser with Text Metrics off
    // (see `metrics_off_app` on CaretHit). Carried into the mirror/centre fallback
    // hits so the overlay can prompt the user to enable it.
    let mut metrics_off_app: Option<String> = None;

    if let Some(focused) = copy_attr_element(system_wide, kAXFocusedUIElementAttribute) {
        let focused_ref = focused.as_concrete_ax();

        // Tier 1: Chromium / Electron text-marker bounds.
        if let Some(rect) = chromium_caret_rect(focused_ref) {
            log_line("coaching: caret tier=chromium");
            return Some(CaretHit { rect: cg_to_logical(rect), centered: false, metrics_off_app: None });
        }

        // Chromium tier failed on a Chromium/WebKit web text field: this is a
        // browser with Text Metrics accessibility disabled. With the flag OFF the
        // element exposes NO geometry attributes (and not even the text-marker
        // range), so we fingerprint the web field by `AXReplaceRangeWithText` — a
        // Chromium/WebKit-specific parameterized attribute present whether or not
        // Text Metrics is on. Record the app so the overlay can prompt the user.
        if has_parameterized_attr(focused_ref, "AXReplaceRangeWithText") {
            metrics_off_app = frontmost_app_name();
        }

        // Tier 2: native AppKit parameterized range bounds.
        if let Some(rect) = native_range_caret_rect(focused_ref) {
            log_line("coaching: caret tier=native_range");
            return Some(CaretHit { rect: cg_to_logical(rect), centered: false, metrics_off_app: None });
        }

        // Tier 3: mirror — focused element frame (position + size). Only trust
        // it when it's small enough to be a real text field; a window-sized
        // frame (e.g. a terminal's content view) is useless as a caret anchor,
        // so we fall through to the screen-centre fallback instead.
        if let Some(rect) = mirror_frame_rect(focused_ref) {
            if !is_empty_rect(&rect) && !is_window_sized(&rect) {
                log_line("coaching: caret tier=mirror");
                return Some(CaretHit { rect: cg_to_logical(rect), centered: false, metrics_off_app });
            }
        }
    }

    // Fallback: no usable caret/field (e.g. Ghostty and other GPU terminals that
    // expose no AX text geometry). Show the overlay at the active screen centre
    // — a predictable, always-visible spot rather than a stale/static position.
    if let Some(center) = screen_center_logical() {
        log_line("coaching: caret tier=center-fallback");
        return Some(CaretHit { rect: center, centered: true, metrics_off_app });
    }

    log_line("coaching: caret tier=none (all AX tiers + center fallback failed)");
    None
}

// ---- Tier 1: Chromium ------------------------------------------------------

fn chromium_caret_rect(element: AXUIElementRef) -> Option<CGRect> {
    // The selected text-marker range (an opaque AXTextMarkerRange), collapsed to a
    // point when there's no selection. Its bounds is the caret rect — zero-width
    // but full line-height — in global screen coords.
    let sel_range = copy_attr_raw(element, AX_SELECTED_TEXT_MARKER_RANGE)?;
    let rect = bounds_for_marker_range(element, sel_range.as_CFTypeRef())?;
    is_usable_caret(&rect).then_some(rect)
}

/// Bounds (AXValue<CGRect>) of an AXTextMarkerRange parameter.
fn bounds_for_marker_range(element: AXUIElementRef, marker_range: CFTypeRef) -> Option<CGRect> {
    let bounds =
        copy_parameterized_value(element, AX_BOUNDS_FOR_TEXT_MARKER_RANGE, marker_range)?;
    ax_value_to_cgrect(bounds.as_concrete_ax_value())
}

// ---- Tier 2: native AppKit range ------------------------------------------

fn native_range_caret_rect(element: AXUIElementRef) -> Option<CGRect> {
    // Insertion point = location of the selected text range (length usually 0).
    let sel = copy_attr_raw(element, kAXSelectedTextRangeAttribute)?;
    let mut sel_range = CFRange {
        location: 0,
        length: 0,
    };
    let ok = unsafe {
        AXValueGetValue(
            sel.as_concrete_ax_value(),
            kAXValueTypeCFRange,
            &mut sel_range as *mut CFRange as *mut c_void,
        )
    };
    if !ok {
        return None;
    }
    let loc = sel_range.location;

    // A length-0 range returns kAXErrorNoValue, so query a single glyph. Caret
    // sits at the LEADING edge of the glyph *at* the insertion point — works for
    // a caret in the middle of text.
    if let Some(rect) = bounds_for_range(element, loc, 1) {
        if is_usable_caret(&rect) {
            return Some(rect);
        }
    }

    // Caret at end of text/line: there is no glyph at `loc`, so the forward query
    // returns no value. Use the PRECEDING glyph and put the caret at its TRAILING
    // edge — this is the common "just typed a character" position.
    if loc > 0 {
        if let Some(rect) = bounds_for_range(element, loc - 1, 1) {
            if is_usable_caret(&rect) {
                return Some(CGRect {
                    origin: CGPoint {
                        x: rect.origin.x + rect.size.width,
                        y: rect.origin.y,
                    },
                    size: CGSize {
                        width: 1.0,
                        height: rect.size.height,
                    },
                });
            }
        }
    }

    None
}

/// Bounds of the text range `{location, length}` on a native AX text element via
/// `kAXBoundsForRangeParameterizedAttribute`. `None` on any AX error.
fn bounds_for_range(element: AXUIElementRef, location: isize, length: isize) -> Option<CGRect> {
    let query = CFRange { location, length };
    let value =
        unsafe { AXValueCreate(kAXValueTypeCFRange, &query as *const CFRange as *const c_void) };
    if value.is_null() {
        return None;
    }
    let _value_guard = unsafe { wrap_cf(value as CFTypeRef) }?;

    let bounds = copy_parameterized_value(
        element,
        kAXBoundsForRangeParameterizedAttribute,
        value as CFTypeRef,
    )?;
    ax_value_to_cgrect(bounds.as_concrete_ax_value())
}

// ---- Tier 3: mirror (field frame) -----------------------------------------

fn mirror_frame_rect(element: AXUIElementRef) -> Option<CGRect> {
    // kAXFrameAttribute is not exported by accessibility-sys 0.2.0, so we read
    // the universally-supported AXPosition (CGPoint) + AXSize (CGSize) and
    // combine them. Both are core attributes present on essentially every
    // focusable element.
    let pos_val = copy_attr_raw(element, kAXPositionAttribute)?;
    let mut origin = CGPoint { x: 0.0, y: 0.0 };
    let ok = unsafe {
        AXValueGetValue(
            pos_val.as_concrete_ax_value(),
            kAXValueTypeCGPoint,
            &mut origin as *mut CGPoint as *mut c_void,
        )
    };
    if !ok {
        return None;
    }

    let size_val = copy_attr_raw(element, kAXSizeAttribute)?;
    let mut size = CGSize {
        width: 0.0,
        height: 0.0,
    };
    let ok = unsafe {
        AXValueGetValue(
            size_val.as_concrete_ax_value(),
            kAXValueTypeCGSize,
            &mut size as *mut CGSize as *mut c_void,
        )
    };
    if !ok {
        return None;
    }

    Some(CGRect { origin, size })
}

// ---- Coordinate conversion -------------------------------------------------

/// Convert an AX/Quartz CGRect to Tauri logical (NS, top-left origin) screen
/// points.
///
/// ASSUMPTION / EMPIRICAL DEFAULT (documented for manual verification):
///   macOS Accessibility reports element geometry in the **global top-left-origin
///   "Quartz/AX" coordinate space, in points (NOT backing pixels)**. The origin
///   is the top-left of the main display; secondary displays placed above/left
///   of it have negative coordinates. Tauri logical screen coords are ALSO
///   top-left-origin device-independent points with the same origin. Therefore
///   the conversion is the IDENTITY — no Y-flip and no /backingScaleFactor
///   scaling is applied. AX does not pre-multiply by the Retina backing factor,
///   so dividing here would shrink positions on Retina displays.
///
///   This is the reasoned default. If manual testing on the user's actual
///   monitor arrangement shows the overlay is offset (e.g. flipped vertically
///   on a multi-monitor setup, or scaled on Retina), correct it here: the
///   `screen_total_height` / `backingScaleFactor` helpers below give the values
///   a flip/scale would need. The single source of truth for the formula is
///   THIS function — the frontend/panel never converts.
fn cg_to_logical(rect: CGRect) -> ScreenRect {
    // Sanity log: which screen (if any) contains the rect origin, plus its
    // backing scale — useful when manually validating the assumption above.
    if let Some((scale, contained)) = screen_for_point(rect.origin.x, rect.origin.y) {
        log_line(&format!(
            "coaching: caret cg=({:.1},{:.1},{:.1},{:.1}) screen_scale={} contained={}",
            rect.origin.x, rect.origin.y, rect.size.width, rect.size.height, scale, contained
        ));
    }
    ScreenRect {
        x: rect.origin.x,
        y: rect.origin.y,
        width: rect.size.width,
        height: rect.size.height,
    }
}

/// Find the backing scale factor of the NSScreen whose frame contains the given
/// AX point, for logging/validation. Returns `(backingScaleFactor, contained)`.
/// NSScreen frames are bottom-left-origin, so we flip the AX (top-left) y into
/// NS space against the main screen height before the contains-check.
fn screen_for_point(ax_x: f64, ax_y: f64) -> Option<(f64, bool)> {
    use objc2_app_kit::NSScreen;
    use objc2_foundation::MainThreadMarker;

    // Safe: locate_caret runs on the main thread (engine dispatches to main).
    let mtm = MainThreadMarker::new()?;
    let screens = NSScreen::screens(mtm);
    let main_height = NSScreen::mainScreen(mtm).map(|s| s.frame().size.height)?;
    // AX y is top-left; NS y is bottom-left. ns_y = main_height - ax_y.
    let ns_y = main_height - ax_y;

    for screen in screens.iter() {
        let f = screen.frame();
        let scale = screen.backingScaleFactor();
        let within_x = ax_x >= f.origin.x && ax_x <= f.origin.x + f.size.width;
        let within_y = ns_y >= f.origin.y && ns_y <= f.origin.y + f.size.height;
        if within_x && within_y {
            return Some((scale, true));
        }
    }
    // Not contained in any screen; report the main screen scale as a fallback.
    let main_scale = NSScreen::mainScreen(mtm).map(|s| s.backingScaleFactor())?;
    Some((main_scale, false))
}

/// True when a frame is large enough to almost certainly be a window/whole view
/// rather than a focused text field (so it's a poor caret anchor). Compared
/// against the main screen size: ≥70% width AND ≥60% height.
fn is_window_sized(rect: &CGRect) -> bool {
    use objc2_app_kit::NSScreen;
    use objc2_foundation::MainThreadMarker;
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(main) = NSScreen::mainScreen(mtm) else {
        return false;
    };
    let sf = main.frame().size;
    sf.width > 0.0
        && sf.height > 0.0
        && rect.size.width >= 0.70 * sf.width
        && rect.size.height >= 0.60 * sf.height
}

/// The centre of the main screen as a zero-size rect in Tauri logical (AX
/// top-left origin) coords. The main screen's top-left is the AX origin (0,0),
/// so its centre is simply (width/2, height/2).
/// Whether `element` supports the named parameterized attribute. Used to
/// fingerprint a Chromium/WebKit web text field independent of Text Metrics.
fn has_parameterized_attr(element: AXUIElementRef, name: &str) -> bool {
    let mut arr: CFArrayRef = std::ptr::null();
    let err = unsafe { AXUIElementCopyParameterizedAttributeNames(element, &mut arr) };
    if err != kAXErrorSuccess || arr.is_null() {
        return false;
    }
    let array = unsafe { CFArray::<*const c_void>::wrap_under_create_rule(arr) };
    array.get_all_values().into_iter().any(|p| {
        !p.is_null() && unsafe { CFString::wrap_under_get_rule(p as CFStringRef) }.to_string() == name
    })
}

/// Localized name of the frontmost application ("Dia", "Arc", …). The focused
/// app stays frontmost because our overlay panel is non-activating. Main thread.
fn frontmost_app_name() -> Option<String> {
    use objc2_app_kit::NSWorkspace;
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    app.localizedName().map(|s| s.to_string())
}

fn screen_center_logical() -> Option<ScreenRect> {
    use objc2_app_kit::NSScreen;
    use objc2_foundation::MainThreadMarker;
    let mtm = MainThreadMarker::new()?;
    let sf = NSScreen::mainScreen(mtm)?.frame().size;
    Some(ScreenRect {
        x: sf.width / 2.0,
        y: sf.height / 2.0,
        width: 0.0,
        height: 0.0,
    })
}

// ---- AX helpers ------------------------------------------------------------

/// A retained AX/CF object that releases on drop. Wraps the raw ref in a
/// `CFType` (create-rule = takes ownership of the +1 retain).
struct AxRef(CFType);

impl AxRef {
    /// Reinterpret the held CFTypeRef as an AXUIElementRef (no extra retain).
    fn as_concrete_ax(&self) -> AXUIElementRef {
        self.0.as_concrete_TypeRef() as AXUIElementRef
    }
    /// Reinterpret the held CFTypeRef as an AXValueRef (no extra retain).
    fn as_concrete_ax_value(&self) -> AXValueRef {
        self.0.as_concrete_TypeRef() as AXValueRef
    }
    #[allow(non_snake_case)]
    fn as_CFTypeRef(&self) -> CFTypeRef {
        self.0.as_concrete_TypeRef()
    }
}

/// Wrap a +1-retained raw CFTypeRef into an owning `CFType`; `None` if null.
unsafe fn wrap_cf(raw: CFTypeRef) -> Option<CFType> {
    if raw.is_null() {
        None
    } else {
        Some(CFType::wrap_under_create_rule(raw))
    }
}

/// Copy an attribute (by name) whose value is itself an AX element; returns an
/// owning wrapper.
fn copy_attr_element(element: AXUIElementRef, name: &str) -> Option<AxRef> {
    copy_attr_raw(element, name)
}

/// Copy an attribute value (by CFString name) into an owning `AxRef`. Handles
/// the +1 retain via the create-rule wrap. `None` on any AX error or null.
fn copy_attr_raw(element: AXUIElementRef, name: &str) -> Option<AxRef> {
    let cf_name = CFString::new(name);
    let mut value: CFTypeRef = std::ptr::null();
    let err: AXError = unsafe {
        AXUIElementCopyAttributeValue(
            element,
            cf_name.as_concrete_TypeRef(),
            &mut value as *mut CFTypeRef,
        )
    };
    if err != kAXErrorSuccess {
        return None;
    }
    unsafe { wrap_cf(value) }.map(AxRef)
}

/// Call a parameterized attribute (by CFString name) with a CFTypeRef parameter;
/// returns the +1-retained result wrapped in an owning `AxRef`.
fn copy_parameterized_value(
    element: AXUIElementRef,
    name: &str,
    parameter: CFTypeRef,
) -> Option<AxRef> {
    let cf_name = CFString::new(name);
    let mut value: CFTypeRef = std::ptr::null();
    let err: AXError = unsafe {
        AXUIElementCopyParameterizedAttributeValue(
            element,
            cf_name.as_concrete_TypeRef(),
            parameter,
            &mut value as *mut CFTypeRef,
        )
    };
    if err != kAXErrorSuccess {
        return None;
    }
    unsafe { wrap_cf(value) }.map(AxRef)
}

/// Extract a CGRect from an AXValue (type kAXValueTypeCGRect).
fn ax_value_to_cgrect(value: AXValueRef) -> Option<CGRect> {
    if value.is_null() {
        return None;
    }
    let mut rect = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize {
            width: 0.0,
            height: 0.0,
        },
    };
    let ok = unsafe {
        AXValueGetValue(
            value,
            kAXValueTypeCGRect,
            &mut rect as *mut CGRect as *mut c_void,
        )
    };
    if ok {
        Some(rect)
    } else {
        None
    }
}

fn is_empty_rect(rect: &CGRect) -> bool {
    rect.size.width <= 0.0 || rect.size.height <= 0.0
}

/// Whether a text-bounds rect is a usable caret anchor. A caret is naturally
/// zero-WIDTH but has positive height (the line height), so we require only
/// height. This rejects the fully-degenerate (0×0) rects some apps return for a
/// caret they don't actually track (e.g. Dia's Chromium build), which would
/// otherwise anchor the overlay to a meaningless fixed point.
fn is_usable_caret(rect: &CGRect) -> bool {
    rect.size.height > 0.0
}
