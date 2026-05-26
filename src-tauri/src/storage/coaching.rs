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
mod tests;
