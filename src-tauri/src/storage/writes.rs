use anyhow::Result;
use rusqlite::params;

use super::Storage;

impl Storage {
    /// Increment a word's frequency, bump last_used, accumulate typing time.
    /// `clean` = true when the occurrence had zero backspace corrections.
    pub fn log_word(&self, word: &str, ts_ms: i64, time_ms: i64, clean: bool) -> Result<()> {
        let clean_inc: i64 = if clean { 1 } else { 0 };
        self.conn.execute(
            "INSERT INTO words(word, frequency, last_used, total_time_ms, clean_count)
             VALUES(?1, 1, ?2, ?3, ?4)
             ON CONFLICT(word) DO UPDATE SET
                frequency     = frequency + 1,
                last_used     = ?2,
                total_time_ms = total_time_ms + ?3,
                clean_count   = clean_count + ?4",
            params![word, ts_ms, time_ms, clean_inc],
        )?;
        Ok(())
    }

    /// Increment a chord phrase's frequency, bump last_used, accumulate time.
    /// `kind` is "chord" (simultaneous burst) or "arpeggio" (sequential in-chordmap burst).
    pub fn log_chord(&self, phrase: &str, ts_ms: i64, time_ms: i64, kind: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO chords(phrase, frequency, last_used, total_time_ms, kind)
             VALUES(?1, 1, ?2, ?3, ?4)
             ON CONFLICT(phrase) DO UPDATE SET
                frequency = frequency + 1,
                last_used = ?2,
                total_time_ms = total_time_ms + ?3,
                kind = ?4",
            params![phrase, ts_ms, time_ms, kind],
        )?;
        Ok(())
    }

    /// Bump the manual-typing counter for a phrase (used for proficiency usage rate).
    pub fn bump_chord_manual(&self, phrase: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO chord_manual(phrase, manual_count) VALUES(?1, 1)
             ON CONFLICT(phrase) DO UPDATE SET manual_count = manual_count + 1",
            params![phrase],
        )?;
        Ok(())
    }

    /// High-confidence error: chord fired then same phrase manually retyped within 5s.
    pub fn bump_chord_error(&self, phrase: &str, ts: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO chord_errors(phrase, error_count, last_error) VALUES(?1, 1, ?2)
             ON CONFLICT(phrase) DO UPDATE SET
               error_count = error_count + 1,
               last_error  = ?2",
            params![phrase, ts],
        )?;
        Ok(())
    }

    /// Lower-confidence deletion: chord fired then N backstrokes deleted it within 3s.
    /// May include intentional edits; tracked separately from high-confidence errors.
    pub fn bump_chord_deletion(&self, phrase: &str, ts: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO chord_errors(phrase, deletion_count, last_error) VALUES(?1, 1, ?2)
             ON CONFLICT(phrase) DO UPDATE SET
               deletion_count = deletion_count + 1,
               last_error     = ?2",
            params![phrase, ts],
        )?;
        Ok(())
    }

    /// Chord confusion: the phrase was deleted and a different chord fired shortly after.
    /// Stored in chord_errors.confusion_count against the DELETED phrase.
    pub fn bump_chord_confusion(&self, phrase: &str, ts: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO chord_errors(phrase, confusion_count, last_error) VALUES(?1, 1, ?2)
             ON CONFLICT(phrase) DO UPDATE SET
               confusion_count = confusion_count + 1,
               last_error      = ?2",
            params![phrase, ts],
        )?;
        Ok(())
    }

    /// Record a split-phrase occurrence: two consecutive manual flushes < 3s apart
    /// whose concatenation is a known word or chord phrase.
    pub fn bump_split_phrase(&self, phrase: &str, ts: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO split_phrases(phrase, split_count, last_split) VALUES(?1, 1, ?2)
             ON CONFLICT(phrase) DO UPDATE SET
               split_count = split_count + 1,
               last_split  = ?2",
            params![phrase, ts],
        )?;
        Ok(())
    }

    /// Record a logged unit: its flush timestamp, character count, and source
    /// ("manual" | "chorded"). WPM is computed at query time from these raw rows,
    /// so the legacy `wpm` column is left at 0.
    pub fn add_wpm_sample(&self, t: i64, chars: i64, source: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO wpm_samples(t, wpm, source, chars) VALUES(?1, 0, ?2, ?3)",
            params![t, source, chars],
        )?;
        Ok(())
    }

    /// Insert or update a session row.
    pub fn upsert_session(
        &self,
        id: i64,
        start: i64,
        end: i64,
        char_count: i64,
        word_count: i64,
    ) -> Result<i64> {
        if id <= 0 {
            self.conn.execute(
                "INSERT INTO sessions(start, end, char_count, word_count) VALUES(?1, ?2, ?3, ?4)",
                params![start, end, char_count, word_count],
            )?;
            Ok(self.conn.last_insert_rowid())
        } else {
            self.conn.execute(
                "UPDATE sessions SET start = ?2, end = ?3, char_count = ?4, word_count = ?5 WHERE id = ?1",
                params![id, start, end, char_count, word_count],
            )?;
            Ok(id)
        }
    }
}
