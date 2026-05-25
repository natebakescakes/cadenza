use rusqlite::params;

use crate::types::{
    ActivityBlock, ChordRecord, Proficiency, Suggestion, WordRecord, WpmSample, WpmSummary,
};

use super::{active_ms, wpm_of, Storage, ESTIMATED_CHORD_MS, SESSION_GAP_MS};
use super::combos::{decode_actions_blob, generate_combos, is_inflected};

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

    pub fn wpm_summary(&self) -> WpmSummary {
        let now = chrono::Utc::now().timestamp_millis();
        let units = self.raw_units(0);

        // Overall active time is the shared denominator. chorded/manual are
        // CONTRIBUTIONS to that same denominator, so chorded + manual == overall.
        let overall_active_min = active_ms(&units) / 60000.0;
        let chars_of = |src: &str| -> i64 {
            units.iter().filter(|u| u.source == src).map(|u| u.chars).sum()
        };
        let total_chars: i64 = units.iter().map(|u| u.chars).sum();
        let contribution = |chars: i64| -> f64 {
            if overall_active_min <= 0.0 {
                0.0
            } else {
                (chars as f64 / 5.0) / overall_active_min
            }
        };

        let overall = contribution(total_chars);
        let chorded = contribution(chars_of("chorded"));
        let manual = contribution(chars_of("manual"));

        // Session = most recent run of activity (gaps > SESSION_GAP_MS split runs).
        let mut session_start_idx = 0;
        for i in 1..units.len() {
            if units[i].t - units[i - 1].t > SESSION_GAP_MS {
                session_start_idx = i;
            }
        }
        let session = wpm_of(&units[session_start_idx..]);

        // Rolling = trailing 60s wall-clock window.
        let rolling = self.rolling_wpm(now);

        WpmSummary {
            rolling,
            session,
            overall,
            chorded,
            manual,
        }
    }

    pub fn wpm_trend(&self, range: &str) -> Vec<WpmSample> {
        let now = chrono::Utc::now().timestamp_millis();
        let (since, bucket_ms) = match range {
            "hour" => (now - 3_600_000, 60_000),       // 1-min buckets
            "day" => (now - 86_400_000, 3_600_000),    // hourly buckets
            "week" => (now - 7 * 86_400_000, 3_600_000), // hourly buckets
            "month" => (now - 30 * 86_400_000, 86_400_000), // daily buckets
            _ => (0, 86_400_000),                       // all: daily buckets
        };
        let units = self.raw_units(since);
        if units.is_empty() {
            return Vec::new();
        }

        // Group ordered units into time buckets. Within each bucket compute WPM
        // from chars/5 over capped-gap active minutes (same algorithm as summary).
        let mut out = Vec::new();
        let mut start = 0usize;
        while start < units.len() {
            let bucket = units[start].t / bucket_ms;
            let mut end = start + 1;
            while end < units.len() && units[end].t / bucket_ms == bucket {
                end += 1;
            }
            let slice = &units[start..end];
            let t = bucket * bucket_ms;

            // Overall series.
            out.push(WpmSample {
                t,
                wpm: wpm_of(slice),
                source: "overall".to_string(),
            });
            // chorded/manual contributions share the bucket's overall denominator.
            let denom_min = active_ms(slice) / 60000.0;
            let contrib = |src: &str| -> f64 {
                let c: i64 = slice.iter().filter(|u| u.source == src).map(|u| u.chars).sum();
                if denom_min <= 0.0 {
                    0.0
                } else {
                    (c as f64 / 5.0) / denom_min
                }
            };
            out.push(WpmSample {
                t,
                wpm: contrib("chorded"),
                source: "chorded".to_string(),
            });
            out.push(WpmSample {
                t,
                wpm: contrib("manual"),
                source: "manual".to_string(),
            });

            start = end;
        }
        out
    }

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

    /// Frequent words (len>=2) NOT already a device chord, ordered by score.
    pub fn suggestions(&self, limit: i64, device_id: &str) -> Vec<Suggestion> {
        let lim = if limit <= 0 { 50 } else { limit };

        // --- 1. Fetch a generous over-set so the inflection post-filter still
        //        leaves enough results after pruning. ---
        let fetch_lim = lim * 4;
        let action_to_group = self.action_to_joystick_group(device_id);

        // Build combo→phrases map from existing device chords for conflict detection.
        let mut combo_to_phrases: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        if let Ok(mut st) = self
            .conn
            .prepare("SELECT phrase, actions FROM device_chords")
        {
            if let Ok(rows) = st.query_map([], |r| {
                let phrase: String = r.get(0)?;
                let blob: Vec<u8> = r.get(1)?;
                Ok((phrase, blob))
            }) {
                for (phrase, blob) in rows.flatten() {
                    let combo = decode_actions_blob(&blob);
                    combo_to_phrases
                        .entry(combo)
                        .or_default()
                        .push(phrase);
                }
            }
        }

        // Build phrase → combo map for existing device chords (for compound suggestions).
        let mut phrase_to_combo: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for (combo, phrases) in &combo_to_phrases {
            for p in phrases {
                phrase_to_combo.insert(p.to_ascii_lowercase(), combo.clone());
            }
        }

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

    /// For each USED device chord phrase (fired or errored at least once):
    /// usage rate, fire/manual/error counts, avg fire time, error rate.
    /// Sorted by error_rate DESC so highest-error chords surface first.
    pub fn proficiency(&self) -> Vec<Proficiency> {
        let mut out = Vec::new();
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT DISTINCT dc.phrase,
                COALESCE(c.frequency, 0),
                COALESCE(c.total_time_ms, 0),
                COALESCE(m.manual_count, 0),
                COALESCE(e.error_count, 0),
                COALESCE(e.deletion_count, 0),
                COALESCE(e.confusion_count, 0)
             FROM device_chords dc
             LEFT JOIN chords c        ON LOWER(c.phrase) = LOWER(dc.phrase)
             LEFT JOIN chord_manual m  ON LOWER(m.phrase) = LOWER(dc.phrase)
             LEFT JOIN chord_errors e  ON LOWER(e.phrase) = LOWER(dc.phrase)
             -- Only include chords the user has actually touched (fired OR errored).
             WHERE COALESCE(c.frequency, 0) + COALESCE(e.error_count, 0)
                 + COALESCE(e.deletion_count, 0) + COALESCE(e.confusion_count, 0) >= 1",
        ) {
            let rows = stmt.query_map([], |r| {
                let phrase: String = r.get(0)?;
                let fired: i64 = r.get(1)?;
                let total: i64 = r.get(2)?;
                let manual: i64 = r.get(3)?;
                let errors: i64 = r.get(4)?;
                let deletions: i64 = r.get(5)?;
                let confusions: i64 = r.get(6)?;

                let usage_denom = fired + manual;
                let usage_rate = if usage_denom > 0 {
                    fired as f64 / usage_denom as f64
                } else {
                    0.0
                };

                let error_denom = fired + errors;
                let error_rate = if error_denom > 0 {
                    errors as f64 / error_denom as f64
                } else {
                    0.0
                };

                let deletion_denom = fired + deletions;
                let deletion_rate = if deletion_denom > 0 {
                    deletions as f64 / deletion_denom as f64
                } else {
                    0.0
                };

                let confusion_denom = fired + confusions;
                let confusion_rate = if confusion_denom > 0 {
                    confusions as f64 / confusion_denom as f64
                } else {
                    0.0
                };

                let avg_fire_ms = if fired > 0 {
                    total as f64 / fired as f64
                } else {
                    0.0
                };
                // Consistency proxy: rises with more fires.
                let consistency = if fired > 0 {
                    (fired as f64 / (fired as f64 + 5.0)).min(1.0)
                } else {
                    0.0
                };
                // Mastered: consistent volume (≈15+ fires via consistency gate),
                // low error/confusion rates, AND clearly preferred over manual typing.
                // Deletion rate excluded: too noisy for reliable mastery gating
                // (pass-through backspaces scapegoat simple high-frequency chords).
                let mastered = consistency >= 0.75
                    && error_rate <= 0.1
                    && confusion_rate <= 0.1
                    && usage_rate >= 0.8;

                Ok(Proficiency {
                    phrase,
                    usage_rate,
                    fired_count: fired,
                    manual_count: manual,
                    avg_fire_ms,
                    consistency,
                    mastered,
                    error_count: errors,
                    error_rate,
                    deletion_count: deletions,
                    deletion_rate,
                    confusion_count: confusions,
                    confusion_rate,
                    combos: Vec::new(), // filled below
                })
            });
            if let Ok(rows) = rows {
                for r in rows.flatten() {
                    out.push(r);
                }
            }
        }

        // Populate combos: for each Proficiency entry look up ALL device_chords
        // rows with a matching phrase and decode each actions BLOB.
        for prof in &mut out {
            if let Ok(mut stmt) = self.conn.prepare(
                "SELECT actions FROM device_chords WHERE LOWER(phrase) = LOWER(?1)",
            ) {
                if let Ok(rows) = stmt.query_map(params![prof.phrase], |r| {
                    r.get::<_, Vec<u8>>(0)
                }) {
                    for blob in rows.flatten() {
                        let combo = decode_actions_blob(&blob);
                        if !combo.is_empty() {
                            prof.combos.push(combo);
                        }
                    }
                }
            }
        }

        // Sort by practice-need score DESC so genuinely hard chords surface first.
        // Error signals (retypes, confusions, deletions) heavily outweigh low adoption
        // so chords with actual struggle evidence appear above mere underuse.
        out.sort_by(|a, b| {
            let score = |p: &Proficiency| -> f64 {
                p.error_count as f64 * 5.0
                    + p.confusion_count as f64 * 3.0
                    + p.deletion_count as f64 * 2.0
                    + (1.0 - p.usage_rate) * 1.0
            };
            let sa = score(a);
            let sb = score(b);
            // Primary: practice_score DESC
            sb.partial_cmp(&sa)
                .unwrap_or(std::cmp::Ordering::Equal)
                // Tiebreak: total frequency DESC
                .then_with(|| {
                    let fa = a.fired_count + a.manual_count;
                    let fb = b.fired_count + b.manual_count;
                    fb.cmp(&fa)
                })
        });

        out
    }
}
