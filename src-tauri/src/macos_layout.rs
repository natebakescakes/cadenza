// macOS keyboard layout snapshot + keycode->string translation.
//
// We capture the current Unicode keyboard layout ONCE on the main thread (where
// the Text Input Source Manager APIs are legal) via
// `TISCopyCurrentKeyboardInputSource` + `TISGetInputSourceProperty(
// kTISPropertyUnicodeKeyLayoutData)`, copy out the raw `UCKeyboardLayout` bytes,
// and keep them. Translation then uses `UCKeyTranslate` against those cached
// bytes — which is safe to call from any thread (here, the main-thread tap
// callback) because it never re-enters TSM.
//
// This is the crux of the crash fix: rdev called the layout-lookup TSM APIs
// from a background thread, tripping `_dispatch_assert_queue_fail` -> SIGTRAP.

#![cfg(target_os = "macos")]

use std::os::raw::{c_void, c_ulong};

use core_foundation::base::TCFType;
use core_foundation::data::{CFData, CFDataRef};
use core_graphics::event::CGEventFlags;

// --- Carbon / HIToolbox FFI ------------------------------------------------

type TISInputSourceRef = *mut c_void;
type OSStatus = i32;
type UInt16 = u16;
type UInt32 = u32;
type UniChar = u16;
type UniCharCount = usize;

// kUCKeyTranslate option: don't update the dead-key state.
const K_UC_KEY_TRANSLATE_NO_DEAD_KEYS_BIT: UInt32 = 0;
const K_UC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK: UInt32 = 1 << K_UC_KEY_TRANSLATE_NO_DEAD_KEYS_BIT;

// kEventKeyDown action for UCKeyTranslate.
const K_UC_KEY_ACTION_DOWN: UInt16 = 0;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn TISCopyCurrentKeyboardInputSource() -> TISInputSourceRef;
    fn TISGetInputSourceProperty(
        source: TISInputSourceRef,
        property_key: *const c_void,
    ) -> *mut c_void;
    static kTISPropertyUnicodeKeyLayoutData: *const c_void;

    fn UCKeyTranslate(
        key_layout_ptr: *const u8,
        virtual_key_code: UInt16,
        key_action: UInt16,
        modifier_key_state: UInt32,
        keyboard_type: UInt32,
        key_translate_options: UInt32,
        dead_key_state: *mut UInt32,
        max_string_length: UniCharCount,
        actual_string_length: *mut UniCharCount,
        unicode_string: *mut UniChar,
    ) -> OSStatus;

    fn LMGetKbdType() -> u8;
}

// Release a CFType we obtained with a "Copy" function.
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// A cached, thread-safe snapshot of the keyboard layout. Holds the raw
/// `UCKeyboardLayout` bytes copied out of the layout `CFData`.
pub struct KeyboardLayout {
    /// Raw bytes of the `UCKeyboardLayout` struct. Empty if capture failed.
    layout_data: Vec<u8>,
    keyboard_type: u32,
}

// The raw bytes are an owned, immutable buffer; safe to share across threads.
unsafe impl Send for KeyboardLayout {}
unsafe impl Sync for KeyboardLayout {}

impl KeyboardLayout {
    /// An empty layout (translation always yields ""). Used as a placeholder
    /// before `current()` runs on the main thread.
    pub fn empty() -> Self {
        Self {
            layout_data: Vec::new(),
            keyboard_type: 0,
        }
    }

    /// Capture the current keyboard layout. MUST be called on the main thread.
    /// Falls back to an empty layout on any failure (translation -> "").
    pub fn current() -> Self {
        unsafe {
            let source = TISCopyCurrentKeyboardInputSource();
            if source.is_null() {
                return Self::empty();
            }
            let data_ptr = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
            if data_ptr.is_null() {
                CFRelease(source as *const c_void);
                return Self::empty();
            }
            // `data_ptr` is a CFDataRef owned by `source` (Get rule, no retain).
            // Copy the bytes out so they outlive `source`.
            let cfdata: CFData = CFData::wrap_under_get_rule(data_ptr as CFDataRef);
            let bytes = cfdata.bytes().to_vec();
            let kbd_type = LMGetKbdType() as u32;
            CFRelease(source as *const c_void);
            Self {
                layout_data: bytes,
                keyboard_type: kbd_type,
            }
        }
    }

    /// Translate a virtual keycode + CGEventFlags into a unicode string using
    /// the cached layout. Returns "" for non-printable keys, dead keys, or if
    /// no layout was captured. Never panics.
    pub fn translate(&self, keycode: u16, flags: CGEventFlags) -> String {
        if self.layout_data.is_empty() {
            return String::new();
        }

        // UCKeyTranslate expects Carbon-style modifier bits in bits 8..15.
        // Build the modifier state from CGEventFlags. Carbon modifier masks:
        //   shiftKey   = 1 << 9  (0x0200)
        //   optionKey  = 1 << 11 (0x0800)
        //   controlKey = 1 << 12 (0x1000)  (we generally don't want ctrl-chars)
        //   alphaLock  = 1 << 10 (0x0400)
        // UCKeyTranslate wants (modifiers >> 8) & 0xFF.
        let mut carbon_modifiers: u32 = 0;
        if flags.contains(CGEventFlags::CGEventFlagShift) {
            carbon_modifiers |= 1 << 9;
        }
        if flags.contains(CGEventFlags::CGEventFlagAlternate) {
            carbon_modifiers |= 1 << 11;
        }
        if flags.contains(CGEventFlags::CGEventFlagAlphaShift) {
            carbon_modifiers |= 1 << 10;
        }
        let modifier_key_state: UInt32 = (carbon_modifiers >> 8) & 0xFF;

        let mut dead_key_state: UInt32 = 0;
        let mut buf: [UniChar; 8] = [0; 8];
        let mut actual_len: UniCharCount = 0;

        let status = unsafe {
            UCKeyTranslate(
                self.layout_data.as_ptr(),
                keycode as UInt16,
                K_UC_KEY_ACTION_DOWN,
                modifier_key_state,
                self.keyboard_type,
                K_UC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK,
                &mut dead_key_state as *mut UInt32,
                buf.len(),
                &mut actual_len as *mut UniCharCount,
                buf.as_mut_ptr(),
            )
        };

        if status != 0 || actual_len == 0 {
            return String::new();
        }
        let len = actual_len.min(buf.len());
        String::from_utf16_lossy(&buf[..len])
    }
}

// Silence unused-type warnings on the type alias used only for documentation.
#[allow(dead_code)]
type _Unused = c_ulong;
