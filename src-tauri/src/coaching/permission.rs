// Phase 3 — Accessibility (AX) permission prompt.
//
// The caret locator (`caret.rs`) needs the process to be AX-trusted; it
// early-returns `None` when untrusted, so a missing permission is non-fatal
// (the overlay just won't position). To get the user there once, we call
// `AXIsProcessTrustedWithOptions({ kAXTrustedCheckOptionPrompt: true })` at
// startup — analogous to how the keylogger calls `CGRequestListenEventAccess`
// for Input Monitoring (keylogger.rs:283-287). On first launch this surfaces
// the system "open Accessibility settings" prompt; on later launches, once
// granted, it is a fast no-op.

use accessibility_sys::{kAXTrustedCheckOptionPrompt, AXIsProcessTrustedWithOptions};
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;

use crate::logging::log_line;

/// Request Accessibility trust, prompting the user once if not yet granted.
/// Non-fatal: logs the resulting trust state and returns it. Safe to call from
/// the main thread during `.setup()`.
pub fn prompt_accessibility_trust() -> bool {
    // Build { kAXTrustedCheckOptionPrompt: kCFBooleanTrue }. The static is the
    // canonical "AXTrustedCheckOptionPrompt" CFString; wrap it without taking
    // ownership (it is a constant, not +1 retained).
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let value = CFBoolean::true_value();
    let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);

    let trusted = unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) };

    if trusted {
        log_line("coaching: Accessibility permission already granted");
    } else {
        log_line(
            "coaching: Accessibility permission NOT granted — prompted user. \
             Overlay positioning will be disabled until granted.",
        );
    }
    trusted
}
