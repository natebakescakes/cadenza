use rusqlite::params;

use crate::types::{ChordRecord, WordRecord};

use super::super::Storage;

impl Storage {
    pub fn list_words(&self, limit: i64, sort_by: &str, search: &str) -> Vec<WordRecord> {
        let order = match sort_by {
            "word" => "word ASC",
            "frequency" => "frequency DESC",
            "last_used" => "last_used DESC",
            "avg_speed" | "avgspeed" | "average_speed" => {
                "(CAST(total_time_ms AS REAL) / MAX(frequency, 1)) ASC"
            }
            "accuracy" => {
                "CAST(COALESCE(clean_count,0) AS REAL) / MAX(frequency, 1) DESC"
            }
            _ => "(LENGTH(word) * frequency) DESC",
        };
        let lim = if limit <= 0 { -1 } else { limit };
        let sql = format!(
            "SELECT word, frequency, last_used, total_time_ms, COALESCE(clean_count, 0)
             FROM words
             WHERE word LIKE ?1
               AND LOWER(word) NOT IN (SELECT word FROM hidden_words)
             ORDER BY {order} LIMIT ?2"
        );
        let like = format!("%{search}%");
        let mut out = Vec::new();
        if let Ok(mut stmt) = self.conn.prepare(&sql) {
            let rows = stmt.query_map(params![like, lim], |r| {
                let word: String = r.get(0)?;
                let frequency: i64 = r.get(1)?;
                let last_used: i64 = r.get(2)?;
                let total: i64 = r.get(3)?;
                let clean: i64 = r.get(4)?;
                let avg = if frequency > 0 {
                    total as f64 / frequency as f64
                } else {
                    0.0
                };
                let accuracy_rate = if frequency > 0 {
                    clean as f64 / frequency as f64
                } else {
                    1.0
                };
                let score = word.chars().count() as i64 * frequency;
                Ok(WordRecord {
                    word,
                    frequency,
                    last_used,
                    avg_speed_ms: avg,
                    score,
                    accuracy_rate,
                })
            });
            if let Ok(rows) = rows {
                for r in rows.flatten() {
                    out.push(r);
                }
            }
        }
        out
    }

    pub fn list_chords(&self, limit: i64, sort_by: &str, search: &str) -> Vec<ChordRecord> {
        let order = match sort_by {
            "phrase" | "chord" => "phrase ASC",
            "frequency" => "frequency DESC",
            "last_used" => "last_used DESC",
            "avg_speed" | "avgspeed" | "average_speed" => {
                "(CAST(total_time_ms AS REAL) / MAX(frequency, 1)) ASC"
            }
            _ => "frequency DESC",
        };
        let lim = if limit <= 0 { -1 } else { limit };
        let sql = format!(
            "SELECT phrase, frequency, last_used, total_time_ms, COALESCE(kind,'chord') FROM chords
             WHERE phrase LIKE ?1 ORDER BY {order} LIMIT ?2"
        );
        let like = format!("%{search}%");
        let mut out = Vec::new();
        if let Ok(mut stmt) = self.conn.prepare(&sql) {
            let rows = stmt.query_map(params![like, lim], |r| {
                let phrase: String = r.get(0)?;
                let frequency: i64 = r.get(1)?;
                let last_used: i64 = r.get(2)?;
                let total: i64 = r.get(3)?;
                let kind: String = r.get(4)?;
                let avg = if frequency > 0 {
                    total as f64 / frequency as f64
                } else {
                    0.0
                };
                Ok(ChordRecord {
                    phrase,
                    frequency,
                    last_used,
                    avg_speed_ms: avg,
                    kind,
                })
            });
            if let Ok(rows) = rows {
                for r in rows.flatten() {
                    out.push(r);
                }
            }
        }
        out
    }
}
