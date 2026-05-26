use tauri::Emitter;

use crate::storage::Storage;
use crate::types::{ChordRecord, WordRecord, WpmSample};
use crate::{EVT_CHORD_LOGGED, EVT_WORD_LOGGED, EVT_WPM};

impl super::Detector {
    pub(super) fn emit_word(&self, word: &str, time_ms: i64, chars: f64, ts: i64) {
        let freq = self.lookup_freq("words", "word", word);
        let clean = self.store.scalar_i64(
            "SELECT COALESCE(clean_count,0) FROM words WHERE word = ?1",
            word,
        );
        let rec = WordRecord {
            word: word.to_string(),
            frequency: freq,
            last_used: ts,
            avg_speed_ms: if freq > 0 {
                self.total_time("words", "word", word) as f64 / freq as f64
            } else {
                time_ms as f64
            },
            score: word.chars().count() as i64 * freq,
            accuracy_rate: if freq > 0 { clean as f64 / freq as f64 } else { 1.0 },
        };
        let _ = self.app.emit(EVT_WORD_LOGGED, &rec);
        self.emit_wpm(time_ms, chars, ts, "manual");
    }

    pub(super) fn emit_chord(&self, phrase: &str, time_ms: i64, chars: f64, ts: i64, kind: &str) {
        let freq = self.lookup_freq("chords", "phrase", phrase);
        let rec = ChordRecord {
            phrase: phrase.to_string(),
            frequency: freq,
            last_used: ts,
            avg_speed_ms: if freq > 0 {
                self.total_time("chords", "phrase", phrase) as f64 / freq as f64
            } else {
                time_ms as f64
            },
            kind: kind.to_string(),
        };
        let _ = self.app.emit(EVT_CHORD_LOGGED, &rec);
        self.emit_wpm(time_ms, chars, ts, "chorded");
    }

    /// Record a logged unit (its real character count + flush time + source) and
    /// emit the live `wpm` event carrying the trailing-60s rolling speed computed
    /// from raw units. WPM is computed at query time, not from a per-burst rate.
    pub(super) fn emit_wpm(&self, _time_ms: i64, chars: f64, ts: i64, source: &str) {
        if chars < 1.0 {
            return;
        }
        let _ = self.store.add_wpm_sample(ts, chars as i64, source);

        // Live number: rolling WPM over the trailing 60s wall-clock window.
        let rolling = self.store.rolling_wpm(ts);
        let sample = WpmSample {
            t: ts,
            wpm: rolling,
            source: "rolling".to_string(),
        };
        let _ = self.app.emit(EVT_WPM, &sample);

        // Write stats for sketchybar widget (atomic tmp→rename, same FS).
        let json = format!("{{\"wpm\":{rolling:.1}}}\n");
        let data_dir = Storage::data_dir();
        let tmp = data_dir.join("sketchybar.json.tmp");
        let dest = data_dir.join("sketchybar.json");
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, &dest);
        }
    }

    pub(super) fn lookup_freq(&self, table: &str, col: &str, key: &str) -> i64 {
        self.store.scalar_i64(
            &format!("SELECT frequency FROM {table} WHERE {col} = ?1"),
            key,
        )
    }

    pub(super) fn total_time(&self, table: &str, col: &str, key: &str) -> i64 {
        self.store.scalar_i64(
            &format!("SELECT total_time_ms FROM {table} WHERE {col} = ?1"),
            key,
        )
    }
}
