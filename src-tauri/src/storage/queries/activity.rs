use rusqlite::params;

use crate::types::ActivityBlock;

use super::super::{wpm_of, Storage};

impl Storage {
    /// Last 24h of activity broken into 5-minute buckets.
    /// Each bucket carries its WPM (capped-gap algorithm) and the words/chords
    /// that were logged in that window (from words.last_used / chords.last_used).
    pub fn recent_blocks(&self) -> Vec<ActivityBlock> {
        const BLOCK_MS: i64 = 5 * 60 * 1000;
        let now = chrono::Utc::now().timestamp_millis();
        let since = now - 86_400_000; // last 24h

        // --- WPM per 5-min bucket (from wpm_samples raw units) ---------------
        let units = self.raw_units(since);
        let mut bucket_wpm: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
        if !units.is_empty() {
            let mut start = 0usize;
            while start < units.len() {
                let bucket = units[start].t / BLOCK_MS;
                let mut end = start + 1;
                while end < units.len() && units[end].t / BLOCK_MS == bucket {
                    end += 1;
                }
                let slice = &units[start..end];
                bucket_wpm.insert(bucket * BLOCK_MS, wpm_of(slice));
                start = end;
            }
        }

        // --- Words per 5-min bucket (from words.last_used) -------------------
        let mut bucket_manual: std::collections::HashMap<i64, Vec<String>> =
            std::collections::HashMap::new();
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT word, last_used FROM words WHERE last_used >= ?1 ORDER BY last_used ASC",
        ) {
            let rows = stmt.query_map(params![since], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            });
            if let Ok(rows) = rows {
                for item in rows.flatten() {
                    let key = (item.1 / BLOCK_MS) * BLOCK_MS;
                    bucket_manual.entry(key).or_default().push(item.0);
                }
            }
        }

        // --- Chords per 5-min bucket (split by kind: "chord" vs "arpeggio") ----
        let mut bucket_chorded: std::collections::HashMap<i64, Vec<String>> =
            std::collections::HashMap::new();
        let mut bucket_arpeggio: std::collections::HashMap<i64, Vec<String>> =
            std::collections::HashMap::new();
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT phrase, last_used, COALESCE(kind,'chord') FROM chords
             WHERE last_used >= ?1 ORDER BY last_used ASC",
        ) {
            let rows = stmt.query_map(params![since], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?))
            });
            if let Ok(rows) = rows {
                for item in rows.flatten() {
                    let key = (item.1 / BLOCK_MS) * BLOCK_MS;
                    if item.2 == "arpeggio" {
                        bucket_arpeggio.entry(key).or_default().push(item.0);
                    } else {
                        bucket_chorded.entry(key).or_default().push(item.0);
                    }
                }
            }
        }

        // --- Merge into sorted blocks (all buckets that have any data) --------
        let mut all_keys: std::collections::HashSet<i64> = std::collections::HashSet::new();
        for k in bucket_wpm.keys() { all_keys.insert(*k); }
        for k in bucket_manual.keys() { all_keys.insert(*k); }
        for k in bucket_chorded.keys() { all_keys.insert(*k); }
        for k in bucket_arpeggio.keys() { all_keys.insert(*k); }

        let mut blocks: Vec<ActivityBlock> = all_keys
            .into_iter()
            .map(|t| ActivityBlock {
                t,
                wpm: bucket_wpm.get(&t).copied().unwrap_or(0.0),
                manual_words: bucket_manual.remove(&t).unwrap_or_default(),
                chorded_words: bucket_chorded.remove(&t).unwrap_or_default(),
                arpeggio_words: bucket_arpeggio.remove(&t).unwrap_or_default(),
            })
            .collect();
        blocks.sort_by_key(|b| std::cmp::Reverse(b.t));
        blocks
    }
}
