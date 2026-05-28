use rusqlite::params;

use crate::types::Suggestion;

use super::super::{Storage, ESTIMATED_CHORD_MS};
use super::super::combos::{generate_combos, is_inflected};

impl Storage {
    /// Frequent words (len>=2) NOT already a device chord, ordered by score.
    pub fn suggestions(&self, limit: i64, device_id: &str) -> Vec<Suggestion> {
        let lim = if limit <= 0 { 50 } else { limit };

        // --- 1. Fetch a generous over-set so the inflection post-filter still
        //        leaves enough results after pruning. ---
        let fetch_lim = lim * 4;
        let action_to_group = self.action_to_joystick_group(device_id);
        let action_mirror = self.action_mirror_map(device_id);
        let action_finger = self.action_finger_map(device_id);
        // Build the combo↔phrase maps from existing device chords. Shared with
        // the coaching mapping lookup (see `combo_maps`).
        let (combo_to_phrases, phrase_to_combo, _hash_to_serialized) = self.combo_maps();

        let mut candidates: Vec<Suggestion> = Vec::new();
        if let Ok(mut stmt) = self.conn.prepare(
            // Clean words only: must contain a letter, and may ONLY contain
            // letters, apostrophes, and hyphens. NOTE: SQLite GLOB '*' is a
            // wildcard (any chars), not a regex quantifier — so we reject via
            // NOT GLOB on a negated char class rather than a positive pattern.
            // Match chord library case-insensitively via LOWER().
            "SELECT word, frequency, total_time_ms FROM words
             WHERE LENGTH(word) >= 2
               AND LENGTH(word) <= 20
               AND frequency >= 1
               AND LOWER(word) NOT IN (SELECT LOWER(phrase) FROM device_chords)
               AND LOWER(word) NOT IN (SELECT word FROM hidden_words)
               AND word GLOB '*[a-zA-Z]*'
               AND word NOT GLOB '*[^a-zA-Z''-]*'
               -- Real words have a healthy vowel ratio; consonant mashes
               -- (vim-motion runs like 'hvkbbbjjkbjlllhy') don't. Require
               -- vowels (aeiouy) >= 25% of letters (apostrophes/hyphens
               -- stripped from the count).
               AND (LENGTH(REPLACE(REPLACE(word,'''',''),'-',''))
                    - LENGTH(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(REPLACE(
                        LOWER(REPLACE(REPLACE(word,'''',''),'-','')),
                        'a',''),'e',''),'i',''),'o',''),'u',''),'y',''))) * 4
                   >= LENGTH(REPLACE(REPLACE(word,'''',''),'-',''))
             ORDER BY (LENGTH(word) * frequency) DESC LIMIT ?1",
        ) {
            let rows = stmt.query_map(params![fetch_lim], |r| {
                let phrase: String = r.get(0)?;
                let frequency: i64 = r.get(1)?;
                let total: i64 = r.get(2)?;
                let avg_manual_ms = if frequency > 0 {
                    total as f64 / frequency as f64
                } else {
                    0.0
                };
                let score = phrase.chars().count() as i64 * frequency;
                let projected_saving_ms =
                    (avg_manual_ms - ESTIMATED_CHORD_MS).max(0.0) * frequency as f64;
                let combos = generate_combos(
                    &phrase,
                    &action_to_group,
                    &action_mirror,
                    &action_finger,
                    &combo_to_phrases,
                    &phrase_to_combo,
                );
                Ok(Suggestion {
                    phrase,
                    frequency,
                    score,
                    avg_manual_ms,
                    projected_saving_ms,
                    combos,
                })
            });
            if let Ok(rows) = rows {
                for r in rows.flatten() {
                    candidates.push(r);
                }
            }
        }

        // Pull split_phrases: phrases the user always types in two manual halves.
        // These are prime chord candidates even when absent from the words table.
        if let Ok(mut st) = self.conn.prepare(
            "SELECT sp.phrase, sp.split_count, COALESCE(w.total_time_ms, 0)
             FROM split_phrases sp
             LEFT JOIN words w ON LOWER(w.word) = LOWER(sp.phrase)
             WHERE LOWER(sp.phrase) NOT IN (SELECT LOWER(phrase) FROM device_chords)
               AND LOWER(sp.phrase) NOT IN (SELECT word FROM hidden_words)
               AND LENGTH(sp.phrase) >= 2
               AND sp.split_count >= 1
             ORDER BY sp.split_count DESC
             LIMIT ?1",
        ) {
            let rows = st.query_map(params![fetch_lim], |r| {
                let phrase: String = r.get(0)?;
                let split_count: i64 = r.get(1)?;
                let total: i64 = r.get(2)?;
                let avg_manual_ms = if split_count > 0 && total > 0 {
                    total as f64 / split_count as f64
                } else {
                    0.0
                };
                let score = phrase.chars().count() as i64 * split_count;
                let projected_saving_ms =
                    (avg_manual_ms - ESTIMATED_CHORD_MS).max(0.0) * split_count as f64;
                let combos = generate_combos(
                    &phrase,
                    &action_to_group,
                    &action_mirror,
                    &action_finger,
                    &combo_to_phrases,
                    &phrase_to_combo,
                );
                Ok(Suggestion {
                    phrase,
                    frequency: split_count,
                    score,
                    avg_manual_ms,
                    projected_saving_ms,
                    combos,
                })
            });
            if let Ok(rows) = rows {
                for r in rows.flatten() {
                    let lower = r.phrase.to_lowercase();
                    if !candidates.iter().any(|c| c.phrase.to_lowercase() == lower) {
                        candidates.push(r);
                    }
                }
            }
        }

        // Re-sort after merging split_phrases so high-score items surface first.
        candidates.sort_by(|a, b| b.score.cmp(&a.score));

        // --- 2. Build "known bases" set: every lowercased word in `words` plus
        //        every lowercased device-chord phrase.  Used by Layer A. ---
        let mut known_bases: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        if let Ok(mut st) = self.conn.prepare("SELECT LOWER(word) FROM words") {
            if let Ok(rows) = st.query_map([], |r| r.get::<_, String>(0)) {
                for w in rows.flatten() {
                    known_bases.insert(w);
                }
            }
        }
        if let Ok(mut st) = self.conn.prepare("SELECT LOWER(phrase) FROM device_chords") {
            if let Ok(rows) = st.query_map([], |r| r.get::<_, String>(0)) {
                for w in rows.flatten() {
                    known_bases.insert(w);
                }
            }
        }

        // --- 3. Inflection post-filter ---
        let out: Vec<Suggestion> = candidates
            .into_iter()
            .filter(|s| !is_inflected(&s.phrase, &known_bases))
            .take(lim as usize)
            .collect();
        out
    }
}
