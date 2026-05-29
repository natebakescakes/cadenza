use crate::types::Proficiency;

use super::super::Storage;
use super::super::combos::decode_actions_blob_with;

impl Storage {
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

        // Populate combos: decode every device_chords actions BLOB ONCE in a
        // single table scan, bucketed by lowercased phrase, then assign to each
        // Proficiency entry. This replaces an N+1 pattern (one prepared statement
        // + LOWER-join probe per proficiency row) that cost seconds on a
        // real-size library. The hash→serialized library map (built once, not
        // per chord) lets compound chords render as "stroke1 -> stroke2". Output
        // is identical: combos still appear in device_chords scan order, one per
        // matching row, with empty decodes skipped.
        let hash_to_serialized = self.hash_to_serialized_map();
        let mut combos_by_phrase: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        if let Ok(mut stmt) = self
            .conn
            .prepare("SELECT phrase, actions FROM device_chords")
        {
            if let Ok(rows) = stmt.query_map([], |r| {
                let phrase: String = r.get(0)?;
                let blob: Vec<u8> = r.get(1)?;
                Ok((phrase, blob))
            }) {
                for (phrase, blob) in rows.flatten() {
                    let combo = decode_actions_blob_with(&blob, &hash_to_serialized);
                    if !combo.is_empty() {
                        combos_by_phrase
                            .entry(phrase.to_lowercase())
                            .or_default()
                            .push(combo);
                    }
                }
            }
        }
        for prof in &mut out {
            if let Some(combos) = combos_by_phrase.get(&prof.phrase.to_lowercase()) {
                prof.combos = combos.clone();
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

#[cfg(test)]
mod tests {
    use super::super::super::combos::decode_actions_blob_with;
    use super::super::super::Storage;
    use rusqlite::params;

    /// Printable-ASCII blobs decode to a stable, sorted " + "-joined combo string
    /// (each byte 0x20–0x7E maps to itself). Insert one and assert proficiency()
    /// surfaces it, exercising the single-pass combo bucketing.
    #[test]
    fn proficiency_populates_combos_in_single_pass() {
        let s = Storage::open_in_memory();
        // "cat" fired enough + low error => exercise mastery math too.
        s.conn
            .execute(
                "INSERT INTO chords (phrase, frequency, total_time_ms) VALUES ('cat', 20, 4000)",
                [],
            )
            .unwrap();
        // Two device_chords rows for the same phrase => two combos, in scan order.
        let blob_a = vec![b'c', b'a', b't'];
        let blob_b = vec![b'a', b't'];
        s.conn
            .execute(
                "INSERT INTO device_chords (phrase, actions, device_id) VALUES (?1, ?2, 'd')",
                params!["cat", blob_a.clone()],
            )
            .unwrap();
        // Second device row for the SAME phrase => its combo is bucketed and
        // appended after the first, in scan order.
        s.conn
            .execute(
                "INSERT INTO device_chords (phrase, actions, device_id) VALUES (?1, ?2, 'd')",
                params!["cat", blob_b.clone()],
            )
            .unwrap();

        let out = s.proficiency();
        assert_eq!(out.len(), 1);
        let p = &out[0];
        assert_eq!(p.phrase, "cat");

        // Combos match a direct decode of each blob, in scan order.
        let map = s.hash_to_serialized_map();
        let expect_a = decode_actions_blob_with(&blob_a, &map);
        let expect_b = decode_actions_blob_with(&blob_b, &map);
        assert_eq!(p.combos, vec![expect_a, expect_b]);
    }

    /// The mastery formula and per-rate denominators must be unchanged: this pins
    /// the exact numbers so any future query edit that alters them fails loudly.
    #[test]
    fn mastery_math_is_exact() {
        let s = Storage::open_in_memory();
        // fired=20, manual=5 => usage_rate = 20/25 = 0.8 (== gate boundary).
        // errors=1 => error_rate = 1/21; confusions=1 => 1/21 (both <= 0.1).
        // consistency = 20/25 = 0.8 (>= 0.75). All gates pass => mastered.
        s.conn
            .execute(
                "INSERT INTO chords (phrase, frequency, total_time_ms) VALUES ('go', 20, 6000)",
                [],
            )
            .unwrap();
        s.conn
            .execute(
                "INSERT INTO chord_manual (phrase, manual_count) VALUES ('go', 5)",
                [],
            )
            .unwrap();
        s.conn
            .execute(
                "INSERT INTO chord_errors (phrase, error_count, deletion_count, confusion_count)
                 VALUES ('go', 1, 2, 1)",
                [],
            )
            .unwrap();
        s.conn
            .execute(
                "INSERT INTO device_chords (phrase, actions, device_id) VALUES ('go', ?1, 'd')",
                params![vec![b'g', b'o']],
            )
            .unwrap();

        let out = s.proficiency();
        assert_eq!(out.len(), 1);
        let p = &out[0];
        assert_eq!(p.fired_count, 20);
        assert_eq!(p.manual_count, 5);
        assert_eq!(p.error_count, 1);
        assert_eq!(p.deletion_count, 2);
        assert_eq!(p.confusion_count, 1);
        assert!((p.usage_rate - 0.8).abs() < 1e-12);
        assert!((p.error_rate - 1.0 / 21.0).abs() < 1e-12);
        assert!((p.deletion_rate - 2.0 / 22.0).abs() < 1e-12);
        assert!((p.confusion_rate - 1.0 / 21.0).abs() < 1e-12);
        assert!((p.consistency - 20.0 / 25.0).abs() < 1e-12);
        assert!((p.avg_fire_ms - 6000.0 / 20.0).abs() < 1e-12);
        assert!(p.mastered, "all gates pass => mastered");
    }
}
