// Coaching-overlay storage methods: mapping lookup (device chord or suggested
// combo), single-phrase mastery metrics (shared gate so it cannot drift from
// `proficiency()`), and the read-only show/suppress/resurface gate.
//
// Lives in the `storage` module so the `pub(super)` combo helpers
// (`decode_actions_blob`, `generate_combos`) are in scope without widening
// their visibility.

use std::collections::HashMap;

use rusqlite::params;

use crate::types::{CoachingCombo, Settings};

use super::combos::{decode_actions_blob, generate_combos};
use super::Storage;

/// Drop duplicate strings while keeping first-seen order.
fn dedupe_preserving_order(items: &mut Vec<String>) {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    items.retain(|s| seen.insert(s.clone()));
}

/// Resolved coaching mapping for a phrase: the primary combo to display, how
/// many *additional* mappings exist (`+N` badge), where it came from, and the
/// full list of candidate combos (primary first) each with any conflicts.
#[derive(Clone, Debug, PartialEq)]
pub struct CoachingMapping {
    pub primary: String,
    pub alt_count: i64,
    /// "device" | "suggested"
    pub source: String,
    /// All candidate combos (primary first); for "suggested" some may conflict
    /// with existing device chords (see each entry's `conflicts`).
    pub combos: Vec<CoachingCombo>,
}

/// Single-phrase mastery metrics, mirroring the per-row math in `proficiency()`
/// (`queries.rs`). Extracted so the mastery gate is computed in exactly one
/// place and cannot drift between the proficiency view and the coaching gate.
pub(super) struct MasteryMetrics {
    pub usage_rate: f64,
    pub error_rate: f64,
    pub confusion_rate: f64,
    pub consistency: f64,
}

impl MasteryMetrics {
    /// The EXACT mastery gate from `proficiency()` (queries.rs):
    /// consistency ≥ 0.75 ∧ error_rate ≤ 0.1 ∧ confusion_rate ≤ 0.1 ∧ usage_rate ≥ 0.8.
    pub(super) fn mastered(&self) -> bool {
        self.consistency >= 0.75
            && self.error_rate <= 0.1
            && self.confusion_rate <= 0.1
            && self.usage_rate >= 0.8
    }
}

impl Storage {
    /// Build the combo↔phrase maps from existing device chords. Shared by
    /// `suggestions()` and `coaching_mapping()` so the (decode + map) builder
    /// block lives in exactly one place.
    ///
    /// Returns `(combo_to_phrases, phrase_to_combo)`:
    /// - `combo_to_phrases`: combo_string → device-chord phrases (conflict lookup).
    /// - `phrase_to_combo`: lowercase device-chord phrase → its combo_string.
    pub(super) fn combo_maps(
        &self,
    ) -> (HashMap<String, Vec<String>>, HashMap<String, String>) {
        let mut combo_to_phrases: HashMap<String, Vec<String>> = HashMap::new();
        if let Ok(mut st) = self.conn.prepare("SELECT phrase, actions FROM device_chords") {
            if let Ok(rows) = st.query_map([], |r| {
                let phrase: String = r.get(0)?;
                let blob: Vec<u8> = r.get(1)?;
                Ok((phrase, blob))
            }) {
                for (phrase, blob) in rows.flatten() {
                    let combo = decode_actions_blob(&blob);
                    combo_to_phrases.entry(combo).or_default().push(phrase);
                }
            }
        }
        let mut phrase_to_combo: HashMap<String, String> = HashMap::new();
        for (combo, phrases) in &combo_to_phrases {
            for p in phrases {
                phrase_to_combo.insert(p.to_ascii_lowercase(), combo.clone());
            }
        }
        (combo_to_phrases, phrase_to_combo)
    }

    /// Resolve the coaching mapping for a manually-typed phrase.
    ///
    /// Device path: if the phrase has ≥1 `device_chords` row (case-insensitive,
    /// same join style as `proficiency()`), decode each via `decode_actions_blob`;
    /// `primary` = first, `alt_count` = rows − 1, `source` = "device".
    ///
    /// Suggestion path: otherwise generate combos via `generate_combos` and
    /// render the primary `ChordCombo` parts to a display string;
    /// `source` = "suggested". `device_id = None` degrades safely (empty joystick
    /// map → unconstrained letter selection).
    ///
    /// Returns `None` when no mapping could be produced (never panics).
    pub fn coaching_mapping(
        &self,
        phrase: &str,
        device_id: Option<&str>,
    ) -> Option<CoachingMapping> {
        // --- Device path: existing device chord(s) for the phrase. ---
        let mut device_combos: Vec<String> = Vec::new();
        if let Ok(mut stmt) = self
            .conn
            .prepare("SELECT actions FROM device_chords WHERE LOWER(phrase) = LOWER(?1)")
        {
            if let Ok(rows) = stmt.query_map(params![phrase], |r| r.get::<_, Vec<u8>>(0)) {
                for blob in rows.flatten() {
                    let combo = decode_actions_blob(&blob);
                    if !combo.is_empty() {
                        device_combos.push(combo);
                    }
                }
            }
        }
        // Dedupe identical combo strings (the chord library can hold duplicate
        // device_chords rows for a phrase, e.g. "if", which would otherwise show
        // the same mapping twice).
        dedupe_preserving_order(&mut device_combos);

        if !device_combos.is_empty() {
            // Device mappings are the user's OWN chords — list them all (primary
            // first), no conflicts.
            let mut combos: Vec<CoachingCombo> = device_combos
                .iter()
                .map(|c| CoachingCombo {
                    combo: c.clone(),
                    conflicts: Vec::new(),
                })
                .collect();

            // Append conflict-free generated alternatives so the user can switch
            // to a more intuitive combo if the current one keeps misfiring.
            let action_to_group = self.action_to_joystick_group(device_id.unwrap_or(""));
            let (combo_to_phrases, phrase_to_combo) = self.combo_maps();
            let generated =
                generate_combos(phrase, &action_to_group, &combo_to_phrases, &phrase_to_combo);
            let mut seen: std::collections::HashSet<String> =
                device_combos.iter().cloned().collect();
            for c in generated {
                if combos.len() >= 4 {
                    break;
                }
                let combo_str = c.parts.join(" → ");
                if combo_str.is_empty() || !seen.insert(combo_str.clone()) {
                    continue;
                }
                if !c.conflicts.is_empty() {
                    continue;
                }
                combos.push(CoachingCombo {
                    combo: combo_str,
                    conflicts: vec![],
                });
            }

            return Some(CoachingMapping {
                primary: device_combos[0].clone(),
                alt_count: combos.len() as i64 - 1,
                source: "device".to_string(),
                combos,
            });
        }

        // --- Suggestion path: generate display-only combos (primary + alts). ---
        // Each generated combo may collide with an existing device chord; carry
        // the conflicting phrase(s) through so the overlay can warn + offer a
        // non-conflicting alternative.
        let action_to_group = self.action_to_joystick_group(device_id.unwrap_or(""));
        let (combo_to_phrases, phrase_to_combo) = self.combo_maps();
        let generated =
            generate_combos(phrase, &action_to_group, &combo_to_phrases, &phrase_to_combo);

        // Render each ChordCombo's parts to a display string and keep its
        // conflicts. Drop any that render empty or duplicate an earlier combo.
        // Cap at 4 options.
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let combos: Vec<CoachingCombo> = generated
            .into_iter()
            .filter_map(|c| {
                let combo = c.parts.join(" → ");
                if combo.is_empty() || !seen.insert(combo.clone()) {
                    None
                } else {
                    Some(CoachingCombo {
                        combo,
                        conflicts: c.conflicts,
                    })
                }
            })
            .take(4)
            .collect();

        let primary = combos.first()?;
        Some(CoachingMapping {
            primary: primary.combo.clone(),
            alt_count: combos.len() as i64 - 1,
            source: "suggested".to_string(),
            combos,
        })
    }

    /// Compute single-phrase mastery metrics, mirroring the per-row math in
    /// `proficiency()`. Reads `chords`/`chord_manual`/`chord_errors` for one
    /// phrase (case-insensitive). Returns zeroed metrics if the phrase is unknown.
    pub(super) fn mastery_metrics(&self, phrase: &str) -> MasteryMetrics {
        let fired = self.scalar_i64(
            "SELECT COALESCE(frequency,0) FROM chords WHERE LOWER(phrase) = LOWER(?1)",
            phrase,
        );
        let manual = self.scalar_i64(
            "SELECT COALESCE(manual_count,0) FROM chord_manual WHERE LOWER(phrase) = LOWER(?1)",
            phrase,
        );
        let errors = self.scalar_i64(
            "SELECT COALESCE(error_count,0) FROM chord_errors WHERE LOWER(phrase) = LOWER(?1)",
            phrase,
        );
        let confusions = self.scalar_i64(
            "SELECT COALESCE(confusion_count,0) FROM chord_errors WHERE LOWER(phrase) = LOWER(?1)",
            phrase,
        );

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
        let confusion_denom = fired + confusions;
        let confusion_rate = if confusion_denom > 0 {
            confusions as f64 / confusion_denom as f64
        } else {
            0.0
        };
        let consistency = if fired > 0 {
            (fired as f64 / (fired as f64 + 5.0)).min(1.0)
        } else {
            0.0
        };

        MasteryMetrics {
            usage_rate,
            error_rate,
            confusion_rate,
            consistency,
        }
    }

    /// Whether a coaching reminder should be shown for `phrase`. READ-ONLY:
    /// never writes `mastered_at` (stamping is the chord-fire path's job).
    ///
    /// `source = "device"`:
    ///   - currently mastered → false (suppress — you've got this chord down).
    ///   - otherwise          → true  (remind; "resurface" after regression is
    ///                                  automatic since a regressed chord is, by
    ///                                  definition, no longer currently mastered).
    /// `source = "suggested"`:
    ///   - words.frequency >= coaching_suggest_min_count → true, else false.
    ///
    /// NOTE: we deliberately show whenever a chord is not *currently* mastered,
    /// rather than only when usage drops below a separate `resurface_rate`. The
    /// latter created a dead band (mastered-then-slightly-regressed words in the
    /// [resurface_rate, mastery) usage range were hidden), which made common
    /// chords stop appearing over a session. `mastered_at` is still stamped on
    /// the fire path (kept for analytics/future use) but no longer gates display.
    pub fn coaching_should_show(&self, phrase: &str, source: &str, settings: &Settings) -> bool {
        // Suppress hints for very short tokens regardless of source. Covers both
        // suggested combos (little chord value) and device reminders (which would
        // otherwise fire when a 2-letter word like "at" collides with a Mouseless
        // grid label). coaching_suggest_min_len defaults to 3.
        if (phrase.chars().count() as i64) < settings.coaching_suggest_min_len {
            return false;
        }

        if source == "suggested" {
            let freq = self.scalar_i64(
                "SELECT COALESCE(frequency,0) FROM words WHERE LOWER(word) = LOWER(?1)",
                phrase,
            );
            return freq >= settings.coaching_suggest_min_count;
        }

        // source == "device". Default: show for EVERY manually-typed chord so
        // it's obvious the feature works. Only suppress currently-mastered chords
        // when the user opts in via `coaching_hide_mastered`.
        if settings.coaching_hide_mastered {
            !self.mastery_metrics(phrase).mastered()
        } else {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn settings() -> Settings {
        // Defaults: coaching_suggest_min_count = 1, coaching_hide_mastered = false.
        Settings::default()
    }

    /// Settings with mastery-suppression ON (for testing the suppress/resurface
    /// gate, which is opt-in via `coaching_hide_mastered`).
    fn settings_hide_mastered() -> Settings {
        Settings {
            coaching_hide_mastered: true,
            ..Settings::default()
        }
    }

    /// Insert a device chord whose actions decode to the given ASCII keys.
    /// `decode_actions_blob` renders printable ASCII bytes directly and sorts
    /// them, so a blob of `[b'b', b'a']` decodes to "a + b".
    fn add_device_chord(s: &Storage, phrase: &str, keys: &[u8], device_id: &str) {
        s.conn
            .execute(
                "INSERT INTO device_chords(phrase, actions, device_id) VALUES(?1, ?2, ?3)",
                params![phrase, keys.to_vec(), device_id],
            )
            .unwrap();
    }

    /// Seed chords/chord_manual/chord_errors counts for one phrase.
    fn seed_metrics(
        s: &Storage,
        phrase: &str,
        fired: i64,
        manual: i64,
        errors: i64,
        confusions: i64,
    ) {
        s.conn
            .execute(
                "INSERT INTO chords(phrase, frequency, last_used, total_time_ms, kind)
                 VALUES(?1, ?2, 0, 0, 'chord')
                 ON CONFLICT(phrase) DO UPDATE SET frequency = ?2",
                params![phrase, fired],
            )
            .unwrap();
        s.conn
            .execute(
                "INSERT INTO chord_manual(phrase, manual_count) VALUES(?1, ?2)
                 ON CONFLICT(phrase) DO UPDATE SET manual_count = ?2",
                params![phrase, manual],
            )
            .unwrap();
        s.conn
            .execute(
                "INSERT INTO chord_errors(phrase, error_count, confusion_count, last_error)
                 VALUES(?1, ?2, ?3, 0)
                 ON CONFLICT(phrase) DO UPDATE SET error_count = ?2, confusion_count = ?3",
                params![phrase, errors, confusions],
            )
            .unwrap();
    }

    fn mastered_at(s: &Storage, phrase: &str) -> Option<i64> {
        s.conn
            .query_row(
                "SELECT mastered_at FROM chord_manual WHERE phrase = ?1",
                params![phrase],
                |r| r.get::<_, Option<i64>>(0),
            )
            .ok()
            .flatten()
    }

    // --- V-Unit1: coaching_mapping (device + suggested + empty) -------------

    #[test]
    fn v_unit1_device_mapping_decodes_primary_and_alt_count() {
        let s = Storage::open_in_memory();
        // Two device_chords rows for the same phrase. Device combos are listed
        // first, then conflict-free generated alternatives are appended (up to 4
        // total). "the" → consonants "h + t" + full "e + h + t" are both
        // conflict-free, so combos = 4, alt_count = 3.
        add_device_chord(&s, "the", &[b'b', b'a'], "dev-1"); // decodes "a + b"
        add_device_chord(&s, "the", &[b'c', b'd'], "dev-1"); // decodes "c + d"

        let m = s
            .coaching_mapping("the", Some("dev-1"))
            .expect("device mapping present");
        assert_eq!(m.source, "device");
        assert_eq!(m.primary, "a + b");
        assert_eq!(m.alt_count, 3);
    }

    #[test]
    fn v_unit1_suggested_mapping_for_chordless_word() {
        let s = Storage::open_in_memory();
        // No device chord for "hello" → suggestion path via generate_combos.
        let m = s
            .coaching_mapping("hello", Some("dev-1"))
            .expect("suggested mapping present");
        assert_eq!(m.source, "suggested");
        assert!(!m.primary.is_empty(), "suggested combo should be non-empty");
        assert_eq!(m.alt_count, 1);
    }

    #[test]
    fn v_unit1_empty_and_no_device_graceful() {
        let s = Storage::open_in_memory();
        // No device, empty-ish input must not panic. A single char yields no
        // usable combo (suggest_chord_combo needs >=1 letter); "" → None.
        assert!(s.coaching_mapping("", None).is_none());
        // A real chordless word with no device still produces a suggestion
        // (joystick map is empty → unconstrained letters).
        let m = s.coaching_mapping("world", None).expect("graceful suggestion");
        assert_eq!(m.source, "suggested");
        assert!(!m.primary.is_empty());
    }

    // --- V-Unit2a: maybe_stamp_mastered on the fire path --------------------

    #[test]
    fn v_unit2a_stamp_on_pass_idempotent_and_no_stamp_on_fail() {
        let s = Storage::open_in_memory();

        // (a) Metrics passing the gate, mastered_at NULL → stamp set to ts.
        // fired=20, manual=0 → usage=1.0, consistency=20/25=0.8, no errors.
        seed_metrics(&s, "pass", 20, 0, 0, 0);
        assert!(s.mastery_metrics("pass").mastered());
        s.maybe_stamp_mastered("pass", 1000).unwrap();
        assert_eq!(mastered_at(&s, "pass"), Some(1000));

        // (b) Call again with a newer ts → mastered_at UNCHANGED (idempotent).
        s.maybe_stamp_mastered("pass", 2000).unwrap();
        assert_eq!(mastered_at(&s, "pass"), Some(1000));

        // (c) Metrics failing the gate, mastered_at NULL → no stamp.
        // fired=2 → consistency=2/7 < 0.75, so not mastered.
        seed_metrics(&s, "fail", 2, 0, 0, 0);
        assert!(!s.mastery_metrics("fail").mastered());
        s.maybe_stamp_mastered("fail", 1000).unwrap();
        assert_eq!(mastered_at(&s, "fail"), None);
    }

    // --- V-Unit2b: coaching_should_show + resurface (READ-only) -------------

    #[test]
    fn v_unit2b_suppress_when_mastered_and_no_write() {
        let s = Storage::open_in_memory();
        seed_metrics(&s, "the", 20, 0, 0, 0); // currently mastered
        assert!(s.mastery_metrics("the").mastered());
        // With hide_mastered ON: mastered → suppressed; gate is read-only (no write).
        assert!(!s.coaching_should_show("the", "device", &settings_hide_mastered()));
        assert_eq!(mastered_at(&s, "the"), None, "gate must not stamp mastered_at");
        // With hide_mastered OFF (default): mastered chords STILL show.
        assert!(s.coaching_should_show("the", "device", &settings()));
    }

    #[test]
    fn v_unit2b_resurface_when_was_mastered_and_usage_dropped() {
        let s = Storage::open_in_memory();
        // Was mastered (mastered_at set), now usage_rate regressed below 0.6.
        // fired=10, manual=20 → usage=10/30=0.333 < 0.6, consistency=10/15=0.667
        // (< 0.75 so NOT currently mastered).
        seed_metrics(&s, "and", 10, 20, 0, 0);
        s.conn
            .execute(
                "UPDATE chord_manual SET mastered_at = 500 WHERE phrase = 'and'",
                [],
            )
            .unwrap();
        assert!(!s.mastery_metrics("and").mastered());
        assert!(s.coaching_should_show("and", "device", &settings()));
    }

    #[test]
    fn v_unit2b_never_mastered_low_usage_uses_normal_branch() {
        let s = Storage::open_in_memory();
        // Never mastered (mastered_at NULL), low usage → normal reminder (true),
        // NOT via the resurface branch.
        seed_metrics(&s, "for", 10, 20, 0, 0); // usage 0.333, not mastered
        assert_eq!(mastered_at(&s, "for"), None);
        assert!(s.coaching_should_show("for", "device", &settings()));
    }

    #[test]
    fn v_unit2b_never_mastered_low_usage_does_not_resurface_path() {
        let s = Storage::open_in_memory();
        // Distinct from resurface: a never-mastered phrase with usage BELOW the
        // resurface_rate must still return true via the NORMAL branch, and must
        // not have a mastered_at written by the gate.
        seed_metrics(&s, "not", 1, 9, 0, 0); // usage 0.1, never mastered
        assert_eq!(mastered_at(&s, "not"), None);
        assert!(s.coaching_should_show("not", "device", &settings()));
        // Confirm read-only: gate never stamps.
        assert_eq!(mastered_at(&s, "not"), None);
    }

    #[test]
    fn v_unit2b_suggested_below_and_above_min_count() {
        let s = Storage::open_in_memory();
        // coaching_suggest_min_count default = 8.
        s.conn
            .execute(
                "INSERT INTO words(word, frequency, last_used, total_time_ms) VALUES('rare', 3, 0, 0)",
                [],
            )
            .unwrap();
        s.conn
            .execute(
                "INSERT INTO words(word, frequency, last_used, total_time_ms) VALUES('common', 12, 0, 0)",
                [],
            )
            .unwrap();
        // Use an explicit min_count = 8 to exercise the threshold (default is 1).
        let s8 = Settings {
            coaching_suggest_min_count: 8,
            ..Settings::default()
        };
        assert!(!s.coaching_should_show("rare", "suggested", &s8));
        assert!(s.coaching_should_show("common", "suggested", &s8));
    }

    #[test]
    fn v_unit2b_below_min_len_suppressed_both_sources() {
        let s = Storage::open_in_memory();
        // A frequent 2-char token (e.g. a Mouseless grid label typed repeatedly):
        // passes the frequency gate but must be suppressed by the length gate.
        s.conn
            .execute(
                "INSERT INTO words(word, frequency, last_used, total_time_ms) VALUES('fj', 50, 0, 0)",
                [],
            )
            .unwrap();
        s.conn
            .execute(
                "INSERT INTO words(word, frequency, last_used, total_time_ms) VALUES('the', 50, 0, 0)",
                [],
            )
            .unwrap();
        // Default min_len = 3: "fj" (len 2) suppressed for BOTH sources.
        assert!(!s.coaching_should_show("fj", "suggested", &settings()));
        assert!(!s.coaching_should_show("fj", "device", &settings()));
        // "the" (len 3) shown for both sources.
        assert!(s.coaching_should_show("the", "suggested", &settings()));
        assert!(s.coaching_should_show("the", "device", &settings()));
        // Lowering min_len to 2 re-enables the 2-char hint.
        let s2 = Settings {
            coaching_suggest_min_len: 2,
            ..Settings::default()
        };
        assert!(s.coaching_should_show("fj", "suggested", &s2));
        assert!(s.coaching_should_show("fj", "device", &s2));
    }

    // --- V-Unit2c: fire → regression arms resurface -------------------------

    #[test]
    fn v_unit2c_fire_then_regression_arms_resurface() {
        let s = Storage::open_in_memory();
        // 1. Drive metrics to mastery on a FIRE so maybe_stamp_mastered stamps it.
        seed_metrics(&s, "with", 20, 0, 0, 0);
        s.maybe_stamp_mastered("with", 777).unwrap();
        assert_eq!(mastered_at(&s, "with"), Some(777));
        // While mastered (with suppression ON), the gate suppresses.
        assert!(!s.coaching_should_show("with", "device", &settings_hide_mastered()));

        // 2. Simulate manual regression: bump manual so usage_rate drops below
        //    the mastery bar. fired=20, manual=60 → usage=0.25 → not mastered.
        seed_metrics(&s, "with", 20, 60, 0, 0);
        s.conn
            .execute(
                "UPDATE chord_manual SET mastered_at = 777 WHERE phrase = 'with'",
                [],
            )
            .unwrap();
        assert!(!s.mastery_metrics("with").mastered());
        // 3. No longer mastered → shows again (resurfaces) even with suppression ON.
        assert!(s.coaching_should_show("with", "device", &settings_hide_mastered()));
    }

    // --- V-Unit3: schema migration adds mastered_at + idempotent ------------

    #[test]
    fn v_unit3_migration_adds_mastered_at_and_is_idempotent() {
        // Build a pre-migration chord_manual table (no mastered_at column).
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE chord_manual (
                phrase TEXT PRIMARY KEY,
                manual_count INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        // Column absent before migration.
        let has_col = |c: &rusqlite::Connection| -> bool {
            c.prepare("SELECT mastered_at FROM chord_manual").is_ok()
        };
        assert!(!has_col(&conn), "mastered_at should not exist pre-migration");

        // Run create_schema → migration adds the column.
        Storage::create_schema_for_test(&conn).unwrap();
        assert!(has_col(&conn), "mastered_at added by migration");

        // Default is NULL for a freshly inserted row.
        conn.execute(
            "INSERT INTO chord_manual(phrase, manual_count) VALUES('x', 1)",
            [],
        )
        .unwrap();
        let val: Option<i64> = conn
            .query_row(
                "SELECT mastered_at FROM chord_manual WHERE phrase = 'x'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(val, None);

        // Re-running create_schema is idempotent (no error on duplicate column).
        Storage::create_schema_for_test(&conn).unwrap();
        assert!(has_col(&conn));
    }
}
