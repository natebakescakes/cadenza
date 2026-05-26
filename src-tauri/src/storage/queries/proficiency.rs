use rusqlite::params;

use crate::types::Proficiency;

use super::super::Storage;
use super::super::combos::decode_actions_blob;

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
