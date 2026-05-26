use std::collections::{HashMap, HashSet};

use crate::types::ChordCombo;

/// Which suffix class matched — used to gate Layer B.
#[derive(PartialEq)]
pub(super) enum SuffixKind {
    /// -er / -ier comparative.  Layer B is DISABLED for this class because
    /// common non-comparatives end in "-er" (water, after, number, paper, …).
    /// We only exclude via Layer A (base actually present in corpus/chordmap).
    ErOnly,
    /// All other inflectional suffixes (-ing, -ed, -s, -es, -ied, -ies).
    /// Both Layer A and Layer B apply.
    Strong,
}

/// Generate candidate root (base) forms for `word` (already lowercased) by
/// inverse-inflecting the longest matching suffix.  Returns `(bases, kind)`;
/// `bases` is empty when no inflectional suffix matches.
///
/// Rules (applied in longest-first order so "-ing" beats "-s"):
///   -ing  → base; base+"e"; de-double final consonant (nn→n, tt→t, …)
///   -ied  → base+"y"
///   -ies  → base+"y"
///   -ed   → base; base (drop "d" only, i.e. base = word[..-1]); de-double
///   -est  → NOT filtered: superlatives have no device special key, so -est
///           words (and false hits like "interest"/"forest") stay as suggestions.
///   -er   → base; base+"e"; de-double; -ier  → base[..-1]+"y"
///           NOTE: Layer B is disabled for -er (see SuffixKind::ErOnly).
///   -es   → base; base (drop just 's')
///   -s    → base (word[..-1]); skip when penultimate char is 's', 'u', 'i'
///           (avoids "kiss","bus","this","axis") or word length ≤ 3.
pub(super) fn candidate_bases(word: &str) -> (Vec<String>, SuffixKind) {
    let b = word.as_bytes();
    let n = b.len();
    let mut bases: Vec<String> = Vec::new();

    // Helper: de-double the final consonant of a stem, e.g. "runn" → "run".
    // Only de-doubles if the last two bytes are the same ASCII letter and that
    // letter is a consonant (not a, e, i, o, u).
    let dedouble = |stem: &str| -> Option<String> {
        let sb = stem.as_bytes();
        let sn = sb.len();
        if sn < 2 {
            return None;
        }
        let c1 = sb[sn - 1];
        let c2 = sb[sn - 2];
        if c1 == c2 && c1.is_ascii_alphabetic() && !b"aeiou".contains(&c1) {
            Some(stem[..sn - 1].to_string())
        } else {
            None
        }
    };

    if n >= 4 && word.ends_with("ing") {
        let stem = &word[..n - 3]; // e.g. "runn", "mak", "typ"
        bases.push(stem.to_string());
        bases.push(format!("{stem}e"));
        if let Some(d) = dedouble(stem) {
            bases.push(d);
        }
        return (bases, SuffixKind::Strong);
    }
    if n >= 5 && word.ends_with("ied") {
        let stem = &word[..n - 3]; // "tr" → "try"
        bases.push(format!("{stem}y"));
        return (bases, SuffixKind::Strong);
    }
    if n >= 5 && word.ends_with("ies") {
        let stem = &word[..n - 3];
        bases.push(format!("{stem}y"));
        return (bases, SuffixKind::Strong);
    }
    // -er: Layer A only (SuffixKind::ErOnly) — avoids nuking "water", "after",
    // "number", "paper", etc., which all pass the length guard but are NOT
    // comparative.  We still generate bases so Layer A can drop "happier" when
    // "happy" is in the corpus.
    if n >= 4 && word.ends_with("er") {
        let stem = &word[..n - 2];
        // comparative: happier→happy (-ier→y)
        if stem.ends_with('i') {
            bases.push(format!("{}y", &stem[..stem.len() - 1]));
        }
        bases.push(stem.to_string());
        bases.push(format!("{stem}e"));
        if let Some(d) = dedouble(stem) {
            bases.push(d);
        }
        return (bases, SuffixKind::ErOnly);
    }
    if n >= 4 && word.ends_with("ed") {
        let stem2 = &word[..n - 2]; // "stopp", "walk"
        let stem1 = &word[..n - 1]; // "use" (just drop the 'd')
        bases.push(stem2.to_string());
        bases.push(stem1.to_string());
        if let Some(d) = dedouble(stem2) {
            bases.push(d);
        }
        return (bases, SuffixKind::Strong);
    }
    if n >= 4 && word.ends_with("es") {
        let stem2 = &word[..n - 2]; // "box"
        let stem1 = &word[..n - 1]; // drop just 's'
        bases.push(stem2.to_string());
        bases.push(stem1.to_string());
        return (bases, SuffixKind::Strong);
    }
    if n >= 4 && word.ends_with('s') {
        // Skip obvious non-plurals: double-s endings (kiss, glass), -us, -is.
        let pen = b[n - 2]; // penultimate byte
        if pen != b's' && pen != b'u' && pen != b'i' {
            bases.push(word[..n - 1].to_string());
            return (bases, SuffixKind::Strong);
        }
    }

    (bases, SuffixKind::Strong) // bases is empty here; kind is unused
}

/// Returns true if `phrase` looks like an inflected form that should be
/// excluded from chord suggestions.  Two-layer heuristic:
///
/// Layer A — base present in corpus/chordmap (high precision):
///   Exclude if any candidate base (≠ phrase) is in `known_bases`.
///   Applies to ALL suffix classes including -er/-ier.
///
/// Layer B — suffix heuristic (covers roots not yet typed):
///   Exclude if candidate_bases() returned bases AND SuffixKind == Strong AND
///   the word length is ≥ 5 AND the shortest candidate base is ≥ 3 letters.
///   (Length guards prevent nuking short words like "sing", "ring", "bus".)
///   DISABLED for -er/-ier (SuffixKind::ErOnly) to protect "water", "after",
///   "number", "paper", and similar common non-comparatives.
pub(super) fn is_inflected(phrase: &str, known_bases: &HashSet<String>) -> bool {
    let w = phrase.to_lowercase();
    let (bases, kind) = candidate_bases(&w);
    if bases.is_empty() {
        return false;
    }

    // Layer A — base known in corpus/chordmap (applies to all suffix classes).
    for base in &bases {
        if base != &w && known_bases.contains(base.as_str()) {
            return true;
        }
    }

    // Layer B — suffix heuristic, disabled for -er/-ier (SuffixKind::ErOnly).
    // Only fires when: Strong suffix, word ≥ 5 chars, shortest base ≥ 3 chars.
    if kind == SuffixKind::Strong {
        let word_long_enough = w.len() >= 5;
        let shortest_base = bases.iter().map(|b| b.len()).min().unwrap_or(0);
        if word_long_enough && shortest_base >= 3 {
            return true;
        }
    }

    false
}

/// Decode a device_chords `actions` BLOB (produced by serial.rs `compress_actions`)
/// back into human-readable key labels, returning one combo string per chord row.
///
/// Encoding: variable-length 8/13-bit values.
///   - If byte > 0 and byte < 32: 13-bit value = (byte << 8) | next_byte.
///   - Otherwise: 8-bit value = byte.
/// For each decoded action code:
///   - 0x00 = padding, skip.
///   - 0x20–0x7E = printable ASCII, render as that character.
///   - Otherwise render as `0xNN` hex label.
/// Simultaneous keys are joined with " + " (sorted for stable display).
/// Never panics on short/malformed input.
pub(super) fn decode_actions_blob(blob: &[u8]) -> String {
    let mut codes: Vec<u16> = Vec::new();
    let mut i = 0;
    while i < blob.len() {
        let byte = blob[i];
        let code: u16 = if byte > 0 && byte < 32 && i + 1 < blob.len() {
            i += 1;
            ((byte as u16) << 8) | (blob[i] as u16)
        } else {
            byte as u16
        };
        i += 1;
        if code != 0 {
            codes.push(code);
        }
    }

    let mut labels: Vec<String> = codes
        .into_iter()
        .map(|c| {
            if (0x20..=0x7e).contains(&c) {
                (c as u8 as char).to_string()
            } else {
                // Known CharaChorder special action codes.
                // Add new entries here as they are identified.
                match c {
                    0x218 => "dup".to_string(),
                    _ => format!("0x{:02X}", c),
                }
            }
        })
        .collect();
    labels.sort(); // stable display order (chords are simultaneous)
    labels.join(" + ")
}

const SUFFIXES: &[&str] = &[
    "ation", "ition", "sion", "ment", "ness", "ance", "ence", "ical",
    "ious", "eous", "tion", "ous", "ive", "ful", "less", "ary", "ery",
    "ory", "ity", "ism", "ist", "ize", "ise", "ally", "ing", "al",
    "ic", "ed", "er", "est", "en", "ly",
];

const PREFIXES: &[&str] = &[
    "under", "over", "anti", "inter", "non", "out",
    "un", "re", "pre", "dis", "mis",
];

const VOWELS: [char; 6] = ['a', 'e', 'i', 'o', 'u', 'y'];

/// Derives a suggested chord key combination for `phrase`, respecting joystick constraints.
///
/// `action_to_group`: maps action_code (e.g. b'a' = 97) to a joystick group id.
/// Keys in the same group cannot be pressed simultaneously and are skipped.
/// When the map is empty (no layout data), joystick constraints are not applied.
///
/// Strategy:
/// 1. All unique letters (in appearance order), filtered by joystick constraint, max 6.
/// 2. If < 2 remain, retry with consonants only.
/// 3. If still < 2, return whatever we have.
fn suggest_chord_combo(
    phrase: &str,
    action_to_group: &HashMap<u16, usize>,
) -> Vec<char> {

    let unique: Vec<char> = {
        let mut seen = HashSet::new();
        phrase
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_lowercase())
            .filter(|c| seen.insert(*c))
            .collect()
    };

    if unique.is_empty() {
        return vec![];
    }

    let pick_valid = |candidates: &[char], max: usize| -> Vec<char> {
        let mut result = Vec::new();
        let mut used_groups: HashSet<usize> = HashSet::new();
        for &c in candidates {
            let code = c as u16;
            if let Some(&group) = action_to_group.get(&code) {
                if !used_groups.insert(group) {
                    continue;
                }
            }
            result.push(c);
            if result.len() >= max {
                break;
            }
        }
        result
    };

    // 1. All unique letters, joystick-filtered, max 6.
    let all_valid = pick_valid(&unique, 6);
    if all_valid.len() >= 2 {
        return all_valid;
    }

    // 2. Consonants only.
    let consonants: Vec<char> = unique
        .iter()
        .filter(|&&c| !VOWELS.contains(&c))
        .copied()
        .collect();
    let cons_valid = pick_valid(&consonants, 6);
    if cons_valid.len() >= 2 {
        return cons_valid;
    }

    // 3. Best effort fallback.
    if all_valid.is_empty() {
        unique.into_iter().take(1).collect()
    } else {
        all_valid
    }
}

/// Generate all chord combo options for `phrase`, ordered: primary single chord first,
/// then compound candidates from suffix/prefix/device-chord-prefix splits.
///
/// Parameters:
/// - `action_to_group`: joystick group map for layout-aware chord generation.
/// - `combo_to_phrases`: map from combo_string → vec of device chord phrases (for conflict lookup).
/// - `phrase_to_combo`: map from lowercase device chord phrase → its combo_string (for existing chord display).
pub(super) fn generate_combos(
    phrase: &str,
    action_to_group: &HashMap<u16, usize>,
    combo_to_phrases: &HashMap<String, Vec<String>>,
    phrase_to_combo: &HashMap<String, String>,
) -> Vec<ChordCombo> {
    let mut results: Vec<ChordCombo> = Vec::new();
    let mut seen_labels: HashSet<String> = HashSet::new();

    let make_combo_string = |chars: Vec<char>| -> String {
        let mut labels: Vec<String> = chars.iter().map(|c| c.to_string()).collect();
        labels.sort();
        labels.join(" + ")
    };

    // --- 1. Primary single chord ---
    let primary_chars = suggest_chord_combo(phrase, action_to_group);
    if !primary_chars.is_empty() {
        let primary_str = make_combo_string(primary_chars);
        let conflicts = combo_to_phrases
            .get(&primary_str)
            .cloned()
            .unwrap_or_default();
        seen_labels.insert(primary_str.clone());
        results.push(ChordCombo {
            kind: "chord".to_string(),
            parts: vec![primary_str],
            conflicts,
        });
    }

    let lower = phrase.to_ascii_lowercase();

    // --- 1.5. Consonants-only chord alternative ---
    // Useful when the primary uses all letters and conflicts with another word
    // (e.g. "leak" and "lake" share the same 4 letters). A consonant-only chord
    // is shorter, often conflict-free, and easy to remember.
    {
        let mut cons_seen = HashSet::new();
        let cons_phrase: String = lower
            .chars()
            .filter(|c| c.is_ascii_alphabetic() && !VOWELS.contains(c) && cons_seen.insert(*c))
            .collect();
        if cons_phrase.len() >= 2 && cons_phrase != lower {
            let cons_chars = suggest_chord_combo(&cons_phrase, action_to_group);
            if cons_chars.len() >= 2 {
                let cons_str = make_combo_string(cons_chars);
                if !cons_str.is_empty() && seen_labels.insert(cons_str.clone()) {
                    let conflicts = combo_to_phrases.get(&cons_str).cloned().unwrap_or_default();
                    results.push(ChordCombo {
                        kind: "chord".to_string(),
                        parts: vec![cons_str],
                        conflicts,
                    });
                }
            }
        }
    }

    // Helper: given a split (stem, affix), produce a compound ChordCombo or None.
    // stem_combo_override: if Some, use this string for the stem part (existing device chord).
    let make_compound = |stem: &str,
                         affix: &str,
                         stem_combo_override: Option<String>|
     -> Option<ChordCombo> {
        if stem.len() < 2 || affix.len() < 2 {
            return None;
        }
        let stem_combo = stem_combo_override.unwrap_or_else(|| {
            make_combo_string(suggest_chord_combo(stem, action_to_group))
        });
        let affix_combo = make_combo_string(suggest_chord_combo(affix, action_to_group));
        if stem_combo.is_empty() || affix_combo.is_empty() {
            return None;
        }
        Some(ChordCombo {
            kind: "compound".to_string(),
            parts: vec![stem_combo, affix_combo],
            conflicts: vec![],
        })
    };

    // --- 2. Device chord prefix match (exact + stem transforms) — highest priority ---
    // Run before generic suffix/prefix heuristics: an existing chord as the stem
    // is a better suggestion than a raw letter-derived split.
    // Sort by length descending so "keyboard" beats "key" for "keyboardist".
    let mut chord_entries: Vec<(&String, &String)> = phrase_to_combo.iter().collect();
    chord_entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    for (existing_phrase, existing_combo) in &chord_entries {
        if results.len() >= 4 { break; }
        if existing_phrase.len() >= lower.len() { continue; }

        let mut stem_forms: Vec<(String, &str)> = Vec::new();
        stem_forms.push((existing_phrase.to_string(), existing_phrase.as_str()));

        let ep = existing_phrase.as_str();
        if ep.len() >= 3 {
            if let Some(last) = ep.chars().last() {
                if matches!(last, 'a' | 'e' | 'i' | 'o' | 'u' | 'y') {
                    let stripped = &ep[..ep.len() - last.len_utf8()];
                    stem_forms.push((stripped.to_string(), ep));
                }
                if last == 'y' {
                    let mut with_i = ep[..ep.len() - 1].to_string();
                    with_i.push('i');
                    stem_forms.push((with_i, ep));
                }
            }
        }

        for (stem_form, display_stem) in stem_forms {
            if results.len() >= 4 { break; }
            if !lower.starts_with(stem_form.as_str()) { continue; }
            let remainder_start = stem_form.len();
            if remainder_start >= phrase.len() { continue; }
            let remainder = &phrase[remainder_start..];
            if remainder.len() < 2 { continue; }
            let remainder_lower = remainder.to_ascii_lowercase();

            // Case A: affix is itself a known device chord — ideal 2-part, show actual chord.
            if let Some(affix_combo) = phrase_to_combo.get(remainder_lower.as_str()) {
                let parts = vec![existing_combo.to_string(), affix_combo.clone()];
                let label = parts.join(" → ");
                if seen_labels.insert(label) {
                    results.push(ChordCombo {
                        kind: "compound".to_string(),
                        parts,
                        conflicts: vec![],
                    });
                }
                continue;
            }

            // Case B: affix starts with a known device chord — 3-part compound.
            // Find the longest device chord that is a proper prefix of the affix.
            let inner = chord_entries.iter().find(|(ep2, _)| {
                remainder_lower.starts_with(ep2.as_str())
                    && remainder_lower.len() > ep2.len()
            });
            if let Some((inner_phrase, inner_combo)) = inner {
                let tail = &remainder[inner_phrase.len()..];
                if tail.len() >= 1 {
                    let tail_combo =
                        make_combo_string(suggest_chord_combo(tail, action_to_group));
                    if !tail_combo.is_empty() {
                        let parts = vec![
                            existing_combo.to_string(),
                            inner_combo.to_string(),
                            tail_combo,
                        ];
                        let label = parts.join(" → ");
                        if seen_labels.insert(label) {
                            results.push(ChordCombo {
                                kind: "compound".to_string(),
                                parts,
                                conflicts: vec![],
                            });
                        }
                        continue;
                    }
                }
            }

            // Case C: generate a chord for the affix.
            if let Some(combo) =
                make_compound(display_stem, remainder, Some(existing_combo.to_string()))
            {
                let label = combo.parts.join(" → ");
                if seen_labels.insert(label) {
                    results.push(combo);
                }
            }
        }
    }

    // --- 3. Suffix splits — fill remaining slots ---
    for &suffix in SUFFIXES {
        if results.len() >= 5 { break; }
        if !lower.ends_with(suffix) { continue; }
        let stem_end = lower.len() - suffix.len();
        if stem_end < 2 { continue; }
        let stem = &phrase[..stem_end];
        let affix = &phrase[stem_end..];
        let stem_override = phrase_to_combo.get(stem.to_ascii_lowercase().as_str()).cloned();
        if let Some(combo) = make_compound(stem, affix, stem_override) {
            let label = combo.parts.join(" → ");
            if seen_labels.insert(label) {
                results.push(combo);
            }
        }
    }

    // --- 4. Prefix splits ---
    for &prefix in PREFIXES {
        if results.len() >= 5 { break; }
        if !lower.starts_with(prefix) { continue; }
        let remainder = &phrase[prefix.len()..];
        if remainder.len() < 2 { continue; }
        let prefix_override = phrase_to_combo.get(prefix).cloned();
        if let Some(combo) = make_compound(prefix, remainder, prefix_override) {
            let label = combo.parts.join(" → ");
            if seen_labels.insert(label) {
                results.push(combo);
            }
        }
    }

    results
}
