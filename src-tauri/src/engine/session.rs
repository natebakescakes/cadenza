impl super::Detector {
    // --- Session tracking -------------------------------------------------

    pub(super) fn update_session(&mut self, ts: i64, chars: i64) {
        if self.session_id == 0 {
            self.session_start = ts;
            self.session_char_count = 0;
            self.session_word_count = 0;
            self.session_id = self
                .store
                .upsert_session(0, ts, ts, 0, 0)
                .unwrap_or(0);
        }
        self.session_last_activity = ts;
        self.session_char_count += chars;
        self.session_word_count += 1;
        let _ = self.store.upsert_session(
            self.session_id,
            self.session_start,
            ts,
            self.session_char_count,
            self.session_word_count,
        );
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
            let _ = self.store.upsert_session(
                self.session_id,
                self.session_start,
                self.session_last_activity,
                self.session_char_count,
                self.session_word_count,
            );
        }
        self.session_id = 0;
    }
}
