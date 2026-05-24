// Global keyboard hook.
//
// macOS (`#[cfg(target_os = "macos")]`):
//   We install a `CGEventTap` (ListenOnly) whose run-loop source is added to
//   the MAIN run loop (`CFRunLoop::get_main()`), which Tauri already runs on
//   the main thread. The tap callback therefore fires on the main thread, where
//   the Text Input Source Manager (TSM) / `UCKeyTranslate` calls used for
//   keycode->string mapping are legal. Running rdev's listener on a background
//   thread aborted the whole process (EXC_BREAKPOINT) because rdev calls TSM
//   off the main thread; see the crash report. We do our OWN keycode->char
//   mapping with a layout snapshot captured once at install time on the main
//   thread, so we never touch TSM from a bad context.
//
// non-macOS (`#[cfg(not(target_os = "macos"))]`):
//   The original `rdev::listen`-on-a-thread implementation, unchanged.
//
// Both paths produce `types::KeyEvent { code, key, pressed, modifiers, ts_ms }`
// pushed into a `crossbeam_channel::Sender<KeyEvent>` consumed by the detector
// thread. The public `KeyLogger` API (`new`, `start`, `resume`, `pause`,
// `is_paused`, `is_running`, `pause_flag`, `last_error`) is identical across
// platforms so `lib.rs`/`commands.rs`/`engine.rs` are unaffected, plus a macOS-
// only `install_main_thread()` hook called from Tauri's `.setup()`.

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::Instant;

    use crossbeam_channel::Sender;
    use parking_lot::Mutex;
    use rdev::{EventType, Key};

    use crate::logging::log_line;
    use crate::types::{KeyEvent, Modifiers};

    /// Owns the keylogger background thread state and pause flag.
    pub struct KeyLogger {
        paused: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
        /// Last error string from `rdev::listen`, if it failed to start.
        pub last_error: Arc<Mutex<Option<String>>>,
    }

    impl KeyLogger {
        pub fn new() -> Self {
            Self {
                paused: Arc::new(AtomicBool::new(true)),
                handle: None,
                last_error: Arc::new(Mutex::new(None)),
            }
        }

        pub fn pause_flag(&self) -> Arc<AtomicBool> {
            self.paused.clone()
        }

        pub fn is_running(&self) -> bool {
            self.handle.is_some()
        }

        /// Spawn the `rdev::listen` thread, forwarding events to `tx`. Idempotent.
        pub fn start(&mut self, tx: Sender<KeyEvent>) {
            if self.handle.is_some() {
                return;
            }
            let paused = self.paused.clone();
            let last_error = self.last_error.clone();
            let handle = std::thread::Builder::new()
                .name("cadenza-keylogger".into())
                .spawn(move || {
                    let origin = Instant::now();
                    let mods = Mutex::new(Modifiers::default());
                    let listen_res = rdev::listen(move |event| {
                        let mut m = mods.lock();
                        update_modifiers(&mut m, &event.event_type);
                        let modifiers = m.clone();
                        drop(m);

                        if paused.load(Ordering::SeqCst) {
                            return;
                        }

                        if let Some(ke) =
                            to_key_event(&event.event_type, &event.name, modifiers, origin)
                        {
                            let _ = tx.send(ke);
                        }
                    });
                    if let Err(e) = listen_res {
                        let msg = format!("{e:?}");
                        log_line(&format!("keylogger(rdev) listen error: {msg}"));
                        *last_error.lock() = Some(msg);
                    }
                })
                .ok();
            self.handle = handle;
        }

        pub fn resume(&self) {
            self.paused.store(false, Ordering::SeqCst);
        }

        pub fn pause(&self) {
            self.paused.store(true, Ordering::SeqCst);
        }

        pub fn is_paused(&self) -> bool {
            self.paused.load(Ordering::SeqCst)
        }
    }

    fn update_modifiers(m: &mut Modifiers, et: &EventType) {
        let (key, pressed) = match et {
            EventType::KeyPress(k) => (*k, true),
            EventType::KeyRelease(k) => (*k, false),
            _ => return,
        };
        match key {
            Key::ControlLeft | Key::ControlRight => m.ctrl = pressed,
            Key::Alt | Key::AltGr => m.alt = pressed,
            Key::ShiftLeft | Key::ShiftRight => m.shift = pressed,
            Key::MetaLeft | Key::MetaRight => m.meta = pressed,
            _ => {}
        }
    }

    fn to_key_event(
        et: &EventType,
        name: &Option<String>,
        modifiers: Modifiers,
        origin: Instant,
    ) -> Option<KeyEvent> {
        let (key, pressed) = match et {
            EventType::KeyPress(k) => (*k, true),
            EventType::KeyRelease(k) => (*k, false),
            _ => return None,
        };
        let ts_ms = origin.elapsed().as_millis() as i64;
        let code = format!("{key:?}");
        let key_str = printable_key(key, name);
        Some(KeyEvent {
            code,
            key: key_str,
            pressed,
            modifiers,
            ts_ms,
        })
    }

    fn printable_key(key: Key, name: &Option<String>) -> String {
        match key {
            Key::Space => " ".to_string(),
            Key::Return | Key::KpReturn => "\n".to_string(),
            Key::Tab => "\t".to_string(),
            Key::Backspace | Key::Delete => "\u{8}".to_string(),
            _ => {
                if let Some(n) = name {
                    if !n.is_empty() {
                        return n.clone();
                    }
                }
                String::new()
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    use crossbeam_channel::Sender;
    use parking_lot::Mutex;

    use core_foundation::base::TCFType;
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
    use core_graphics::event::{
        CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
        CGEventType, CallbackResult, EventField,
    };

    use crate::logging::log_line;
    use crate::macos_layout::KeyboardLayout;
    use crate::types::{KeyEvent, Modifiers};

    // `CGEventTapEnable`/`CGEventTapIsEnabled` are public CoreGraphics symbols
    // but the `core-graphics` crate does not re-export them, so bind directly.
    // `tap` is a `CFMachPortRef` (opaque); we pass the raw pointer we stash.
    extern "C" {
        fn CGEventTapEnable(tap: *const std::ffi::c_void, enable: bool);
    }

    /// Shared state the CGEventTap callback reads. Held in `Arc`s so it stays
    /// alive for the tap's (leaked) lifetime and is cheaply clonable.
    struct TapContext {
        /// Lazily-set destination for key events. The tap is installed during
        /// `.setup()` (possibly before the detector/channel exist), so the
        /// Sender is filled in by `start()`.
        tx: Mutex<Option<Sender<KeyEvent>>>,
        /// Drop events while paused (same semantics as the rdev path).
        paused: Arc<AtomicBool>,
        /// Keyboard layout snapshot captured once on the main thread at install.
        layout: KeyboardLayout,
        /// Monotonic origin for `ts_ms`.
        origin: Instant,
    }

    /// Owns the keylogger control state. On macOS the actual CGEventTap is
    /// created/installed on the main thread via `install_main_thread()` and
    /// leaked (lives for the app lifetime); this struct only holds the shared
    /// context + the raw mach-port pointer used to enable/disable the tap.
    pub struct KeyLogger {
        paused: Arc<AtomicBool>,
        ctx: Arc<TapContext>,
        /// Raw `CFMachPortRef` of the installed tap, as `usize` so the struct
        /// stays `Send`. 0 = not installed yet (or install failed).
        mach_port: Arc<std::sync::atomic::AtomicUsize>,
        /// Whether the tap has been successfully installed on the main run loop.
        installed: Arc<AtomicBool>,
        pub last_error: Arc<Mutex<Option<String>>>,
    }

    impl KeyLogger {
        pub fn new() -> Self {
            let paused = Arc::new(AtomicBool::new(true));
            let ctx = Arc::new(TapContext {
                tx: Mutex::new(None),
                paused: paused.clone(),
                // Capture the layout lazily at install (main thread). A default
                // here is fine; `install_main_thread` re-captures it.
                layout: KeyboardLayout::empty(),
                origin: Instant::now(),
            });
            Self {
                paused,
                ctx,
                mach_port: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                installed: Arc::new(AtomicBool::new(false)),
                last_error: Arc::new(Mutex::new(None)),
            }
        }

        pub fn pause_flag(&self) -> Arc<AtomicBool> {
            self.paused.clone()
        }

        /// On macOS "running" means the tap is installed on the main run loop.
        pub fn is_running(&self) -> bool {
            self.installed.load(Ordering::SeqCst)
        }

        /// MUST be called from the MAIN thread (Tauri `.setup()` closure).
        /// Captures the current keyboard layout, creates a ListenOnly
        /// `CGEventTap`, adds its run-loop source to the MAIN run loop, and
        /// leaves it DISABLED until `start()` enables it. The tap is leaked so
        /// its callback stays valid for the app's lifetime; we keep only the
        /// raw mach-port pointer for enable/disable.
        ///
        /// Returns true on success. On failure (no Accessibility permission ->
        /// `CGEventTap::new` returns Err) records `last_error` and returns false
        /// WITHOUT crashing.
        pub fn install_main_thread(&mut self) {
            if self.installed.load(Ordering::SeqCst) {
                return;
            }

            // Re-capture the layout on the main thread (TSM-safe here).
            let layout = KeyboardLayout::current();
            // Rebuild ctx with the real layout while preserving paused/origin.
            let ctx = Arc::new(TapContext {
                tx: Mutex::new(self.ctx.tx.lock().take()),
                paused: self.paused.clone(),
                layout,
                origin: self.ctx.origin,
            });
            self.ctx = ctx.clone();

            let cb_ctx = ctx.clone();
            let tap_res = CGEventTap::new(
                CGEventTapLocation::HID,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::ListenOnly,
                vec![
                    CGEventType::KeyDown,
                    CGEventType::KeyUp,
                    CGEventType::FlagsChanged,
                ],
                move |_proxy, etype, event| {
                    // Belt-and-suspenders: never let a panic cross the CF/C
                    // callback boundary (that would abort the process again).
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        handle_event(&cb_ctx, etype, event);
                    }));
                    CallbackResult::Keep
                },
            );

            let tap = match tap_res {
                Ok(t) => t,
                Err(()) => {
                    let msg = "CGEventTap creation failed (Accessibility permission \
                               not granted?). Logging will not capture keys until \
                               permission is granted and the app is restarted."
                        .to_string();
                    log_line(&format!("keylogger: {msg}"));
                    *self.last_error.lock() = Some(msg);
                    return;
                }
            };

            // Stash the raw mach-port pointer for enable/disable BEFORE leaking.
            let port_ptr =
                tap.mach_port().as_concrete_TypeRef() as *const std::ffi::c_void as usize;

            // Add the tap's run-loop source to the MAIN run loop. We are on the
            // main thread, so `get_current()` == `get_main()`; use main
            // explicitly to be unambiguous.
            match tap.mach_port().create_runloop_source(0) {
                Ok(source) => {
                    let run_loop = CFRunLoop::get_main();
                    run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });
                }
                Err(()) => {
                    let msg = "failed to create run-loop source for CGEventTap".to_string();
                    log_line(&format!("keylogger: {msg}"));
                    *self.last_error.lock() = Some(msg);
                    return;
                }
            }

            // Leak the tap so its callback closure lives forever (we only ever
            // have one for the app lifetime). Disabled until start().
            std::mem::forget(tap);
            unsafe { CGEventTapEnable(port_ptr as *const std::ffi::c_void, false) };

            self.mach_port.store(port_ptr, Ordering::SeqCst);
            self.installed.store(true, Ordering::SeqCst);
            *self.last_error.lock() = None;
            log_line("keylogger: CGEventTap installed on main run loop (disabled)");
        }

        /// Provide/refresh the Sender the callback forwards events to, and
        /// enable the tap. On macOS the tap was already installed during setup;
        /// this just wires up the channel and enables capture. Idempotent.
        pub fn start(&mut self, tx: Sender<KeyEvent>) {
            *self.ctx.tx.lock() = Some(tx);
            self.enable_tap(true);
        }

        pub fn resume(&self) {
            self.paused.store(false, Ordering::SeqCst);
            self.enable_tap(true);
        }

        pub fn pause(&self) {
            self.paused.store(true, Ordering::SeqCst);
            self.enable_tap(false);
        }

        pub fn is_paused(&self) -> bool {
            self.paused.load(Ordering::SeqCst)
        }

        fn enable_tap(&self, enable: bool) {
            let port = self.mach_port.load(Ordering::SeqCst);
            if port == 0 {
                return; // not installed (permission missing) -> no-op.
            }
            unsafe { CGEventTapEnable(port as *const std::ffi::c_void, enable) };
        }
    }

    /// Translate one CGEvent into a `KeyEvent` and forward it. Never panics.
    fn handle_event(ctx: &TapContext, etype: CGEventType, event: &core_graphics::event::CGEvent) {
        let pressed = match etype {
            CGEventType::KeyDown => true,
            CGEventType::KeyUp => false,
            // Modifier-only changes: update nothing here; modifier state is read
            // per-event from flags below, and a lone modifier maps to empty key
            // (engine treats empty key as a boundary, same as before).
            CGEventType::FlagsChanged => false,
            // Tap auto-disabled by timeout/user input: re-enable so we keep
            // capturing. (Requires the mach port; we don't have it here, but the
            // KeyLogger re-enables on next start/resume. Just log it.)
            CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
                log_line("keylogger: CGEventTap was disabled by the system (timeout/user input)");
                return;
            }
            _ => return,
        };

        let flags = event.get_flags();
        let modifiers = Modifiers {
            ctrl: flags.contains(CGEventFlags::CGEventFlagControl),
            alt: flags.contains(CGEventFlags::CGEventFlagAlternate),
            shift: flags.contains(CGEventFlags::CGEventFlagShift),
            meta: flags.contains(CGEventFlags::CGEventFlagCommand),
        };

        // FlagsChanged events are modifier transitions; emit an empty-key
        // boundary event only on the "press" edge is unnecessary — the engine
        // ignores empty keys. We simply skip emitting for FlagsChanged.
        if matches!(etype, CGEventType::FlagsChanged) {
            return;
        }

        // Filter autorepeat keydowns so we don't inflate counts.
        if pressed && event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) != 0 {
            return;
        }

        // Drop (but having already consumed) events while paused.
        if ctx.paused.load(Ordering::SeqCst) {
            return;
        }

        let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
        let key = map_key(ctx, keycode, flags);
        let code = format!("kc{keycode}");
        let ts_ms = ctx.origin.elapsed().as_millis() as i64;

        let ev = KeyEvent {
            code,
            key,
            pressed,
            modifiers,
            ts_ms,
        };

        if let Some(tx) = ctx.tx.lock().as_ref() {
            let _ = tx.send(ev);
        }
    }

    /// Map a virtual keycode + modifier flags to the engine's `key` string.
    /// Special keys are mapped explicitly to match the existing contract;
    /// everything else is resolved via the cached layout (UCKeyTranslate).
    fn map_key(ctx: &TapContext, keycode: u16, flags: CGEventFlags) -> String {
        // Special keys (match the existing engine contract exactly).
        match keycode {
            0x31 => return " ".to_string(),    // Space
            0x24 | 0x4C => return "\n".to_string(), // Return, KpReturn
            0x30 => return "\t".to_string(),   // Tab
            0x33 => return "\u{8}".to_string(), // Delete/Backspace
            // Non-character keys -> empty (engine treats as boundary/ignored).
            0x75 => return "\u{8}".to_string(), // Forward Delete -> treated as backspace
            0x35 // Escape
            | 0x36 | 0x37 // R/L Command
            | 0x38 | 0x3C // L/R Shift
            | 0x39 // Caps Lock
            | 0x3A | 0x3D // L/R Option
            | 0x3B | 0x3E // L/R Control
            | 0x3F // Function
            | 0x7B | 0x7C | 0x7D | 0x7E // arrows
            | 0x72 | 0x73 | 0x74 | 0x77 | 0x79 // Help/Home/PageUp/End/PageDown
            => return String::new(),
            _ => {}
        }
        // Function keys F1-F20 (0x60..=0x6F plus a few others) and the rest of
        // the non-printables fall through to UCKeyTranslate, which returns an
        // empty string for them. That's fine: empty -> boundary/ignored.
        ctx.layout.translate(keycode, flags)
    }
}

pub use imp::KeyLogger;

impl Default for KeyLogger {
    fn default() -> Self {
        Self::new()
    }
}
