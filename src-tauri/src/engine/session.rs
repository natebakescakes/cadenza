impl super::Detector {
    // --- Session tracking -------------------------------------------------

    /// Heartbeat interval (ms): the session row is flushed at most this often via
    /// the periodic path, not on every word.
    const SESSION_HEARTBEAT_MS: i64 = 30_000;
    /// Also flush after this many words even if the heartbeat hasn't elapsed, so
    /// a fast typist's in-progress session isn't too stale on crash.
    const SESSION_HEARTBEAT_WORDS: i64 = 50;

    pub(super) fn update_session(&mut self, ts: i64, chars: i64) {
        let is_new = self.session_id == 0;
        if is_new {
            self.session_start = ts;
            self.session_char_count = 0;
            self.session_word_count = 0;
            self.session_id = self
                .store
                .upsert_session(0, ts, ts, 0, 0)
                .unwrap_or(0);
            // The INSERT above already wrote the row; record the write so the
            // heartbeat clock starts now.
            self.session_last_write_ts = ts;
            self.session_last_write_word_count = 0;
        }
        self.session_last_activity = ts;
        self.session_char_count += chars;
        self.session_word_count += 1;

        // Throttle the UPDATE: every word previously did a full session-row
        // UPDATE. The live counts are tracked in memory, so the DB row only needs
        // periodic flushes (heartbeat) plus the start (handled above) and close
        // (forced in close_session). Flush when the heartbeat interval elapsed OR
        // every N words.
        let heartbeat_due = ts - self.session_last_write_ts >= Self::SESSION_HEARTBEAT_MS;
        let words_due = self.session_word_count - self.session_last_write_word_count
            >= Self::SESSION_HEARTBEAT_WORDS;
        if !is_new && (heartbeat_due || words_due) {
            let _ = self.store.upsert_session(
                self.session_id,
                self.session_start,
                ts,
                self.session_char_count,
                self.session_word_count,
            );
            self.session_last_write_ts = ts;
            self.session_last_write_word_count = self.session_word_count;
        }
    }

    /// Close the session if idle gap exceeds the new-word threshold.
    pub(super) fn maybe_close_session(&mut self, now: i64) {
        if self.session_id == 0 {
            return;
        }
        let gap_ms = (self.cfg().new_word_threshold_s * 1000.0) as i64;
        if now - self.session_last_activity >= gap_ms {
            self.close_session();
        }
    }

    pub(super) fn close_session(&mut self) {
        if self.session_id != 0 {
            // Force a final, unconditional write so the throttled heartbeat can
            // never lose the session's last words on close.
            let _ = self.store.upsert_session(
                self.session_id,
                self.session_start,
                self.session_last_activity,
                self.session_char_count,
                self.session_word_count,
            );
        }
        self.session_id = 0;
        self.session_last_write_ts = 0;
        self.session_last_write_word_count = 0;
    }
}
