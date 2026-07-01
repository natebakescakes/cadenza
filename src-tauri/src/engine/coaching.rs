use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::Emitter;

use crate::types::{CoachingHint, Settings};
use crate::{EVT_COACHING_DISMISS, EVT_COACHING_HINT};

impl super::Detector {
    /// On a manual flush: resolve the coaching mapping + gate it, and if shown,
    /// emit `coaching_hint` immediately (fire-and-forget) then schedule the
    /// main-thread AX locate (Phase 2 stub) via GCD. Also arms the gated
    /// `EVT_KEYSTROKE` producer + a backend self-clearing timer.
    pub(super) fn maybe_emit_coaching(&mut self, phrase: &str, cfg: &Settings) {
        // NOTE: the mapping is computed + cached below EVEN WHEN `coaching_enabled`
        // is false, so the force-show hotkey can display the last word's chord
        // suggestion while the automatic overlay is switched off. The
        // `coaching_enabled` toggle only gates the AUTO-show (further down).
        //
        // Resolve device_id LIVE from shared state; clone to an owned Option and
        // DROP the guard before any dispatch.
        let device_id: Option<String> = {
            let guard = self.device.lock();
            guard.as_ref().map(|d| format!("{}-{}", d.name, d.version))
        };

        // Ensure the phrase-independent chord maps are cached for this device,
        // rebuilding from SQL only on first use / device change / chordmap
        // refresh. Then resolve the mapping using the cache (only the phrase's
        // own device_chords lookup hits SQL) — avoids 3 layout queries + 2 full
        // device_chords scans on every manual word.
        self.ensure_chord_maps(device_id.as_deref());
        let maps = self.cached_maps.as_ref().expect("cache populated above");
        let mapping = match self.store.coaching_mapping_with(phrase, maps) {
            Some(m) => m,
            None => return,
        };

        // Bump the monotonic hint id and build the hint. The counter is
        // process-global (shared from AppState) so ids keep climbing across
        // detector respawns — a per-Detector counter would reset to 0 and its
        // positions would be dropped by the listener's high-water mark.
        let id = super::next_hint_id(&self.hint_seq);
        let hint = CoachingHint {
            id,
            phrase: phrase.to_string(),
            primary_combo: mapping.primary,
            alt_count: mapping.alt_count,
            source: mapping.source,
            combos: mapping.combos,
            persist: cfg.coaching_persist,
            show_ms: cfg.coaching_show_ms,
            fade_ms: cfg.coaching_fade_ms,
        };

        // Cache the COMPUTED hint before any show-gate, so the force-show hotkey
        // can resurface it even when the auto-overlay is off or gated below.
        *self.last_coaching_hint.lock() = Some(hint.clone());

        // Auto-show gate: when the overlay is switched off, stop here — nothing
        // flashes automatically, but the hotkey can still show the cached hint.
        if !cfg.coaching_enabled {
            return;
        }

        // Show-gate: suppress the auto-overlay for mastered/too-frequent phrases.
        // The hint is already cached above, so force-show still works.
        if !self
            .store
            .coaching_should_show(phrase, &hint.source, cfg)
        {
            return;
        }

        // Publish the latest hint id BEFORE scheduling the async caret locate so a
        // locate already queued for a superseded hint can coalesce itself out
        // (see the main-thread closure below).
        self.latest_hint_id.store(id, Ordering::Relaxed);
        self.coaching_overlay_visible.store(true, Ordering::Relaxed);
        // Debug builds only — fires on every coaching hint (per manual word).
        #[cfg(debug_assertions)]
        crate::logging::log_line(&format!(
            "[COACH] show id={} phrase=\"{}\" source={} persist={}",
            id, phrase, hint.source, cfg.coaching_persist
        ));
        let _ = self.app.emit(EVT_COACHING_HINT, &hint);

        // Schedule the async caret locate on the main thread (where AX is legal).
        // Fire-and-forget: the Detector never awaits it. locate_caret runs the
        // tiered AX locator and returns None if no caret rect can be resolved.
        #[cfg(target_os = "macos")]
        {
            let app = self.app.clone();
            let latest = self.latest_hint_id.clone();
            dispatch2::DispatchQueue::main().exec_async(move || {
                // Coalesce: under fast typing many locate closures queue on the
                // main thread. If a newer hint has already fired, skip the
                // (potentially blocking) AX work entirely — otherwise the queue
                // backs up, stalls the main run loop (and the CGEventTap with it),
                // and positions arrive too late to be honored.
                if latest.load(Ordering::Relaxed) != id {
                    return;
                }
                if let Some(hit) = crate::coaching::locate_caret() {
                    let pos = crate::types::CoachingPosition {
                        id,
                        rect: hit.rect,
                        centered: hit.centered,
                        text_metrics_app: hit.metrics_off_app,
                    };
                    let _ = app.emit(crate::EVT_COACHING_POSITION, &pos);
                }
            });
        }

        // Persist mode: no auto-dismiss. The overlay stays until the NEXT hint
        // replaces it (the frontend also skips its fade + keystroke-dismiss).
        if cfg.coaching_persist {
            return;
        }

        // Backend self-clearing timer (authoritative floor): clear the visible
        // flag after show+fade UNLESS a newer hint has fired in the meantime.
        // Uses tauri's async runtime + a tokio sleep instead of spawning a fresh
        // OS thread per hint (unbounded thread growth under fast typing).
        let flag = self.coaching_overlay_visible.clone();
        let dur_ms = (cfg.coaching_show_ms + cfg.coaching_fade_ms).max(0.0);
        let latest = self.latest_hint_id.clone();
        let timer_app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(dur_ms as u64)).await;
            // Only clear if no newer hint fired while we slept.
            if latest.load(Ordering::Relaxed) == id {
                flag.store(false, Ordering::Relaxed);
                // Authoritative floor: tell the overlay to dismiss too. The
                // frontend's dismiss handler hides the React content AND calls
                // `hide_overlay`, so the NSPanel can't linger as an empty panel.
                // Idempotent: a frontend-driven hide may have already fired.
                let _ = timer_app.emit(EVT_COACHING_DISMISS, ());
            }
        });
    }
}
