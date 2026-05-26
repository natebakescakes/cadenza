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

/// Map a single CharaChorder action code to a short, human-readable label.
///
/// Codes come from the device's keymap (mirrors the categories in
/// CharaChorder DeviceManager's `assets/keymaps/*.yml`):
///   - 0x20–0x7E  printable ASCII → the character itself.
///   - 256–511    OS keyboard scancodes (256 + USB-HID usage): letters, digits,
///                and named keys (enter, arrows, F-keys, …).
///   - 512–524    keyboard modifiers + release/press-next controls.
///   - 528–559    CharaChorder-specific actions (dup, spur, gtm, keymaps, …).
///   - 576–579    action delays.
/// Anything not recognised (reserved/newer-firmware codes) falls back to `0xNN`.
fn action_label(c: u16) -> String {
    if (0x20..=0x7e).contains(&c) {
        return (c as u8 as char).to_string();
    }
    let named = match c {
        // Keyboard modifiers (Left/Right collapse to one label).
        512 | 516 => "ctrl",
        513 | 517 => "shift",
        514 | 518 => "alt",
        515 | 519 => "cmd",
        520 => "rel-mod",
        521 => "rel-all",
        522 => "rel-keys",
        523 => "press-next",
        524 => "rel-next",
        // CharaChorder-specific actions.
        528 => "restart",
        530 => "boot",
        532 => "gtm",
        534 => "impulse",
        536 => "dup",
        538 => "spur",
        540 => "ambi-l",
        542 => "ambi-r",
        544 => "space",
        548 | 549 => "km1",
        550 | 551 => "km2",
        552 | 553 => "km3",
        558 => "hold-lib",
        559 => "base-lib",
        576..=579 => "delay",
        // Named keyboard scancodes.
        296 => "enter",
        297 => "esc",
        298 => "bksp",
        299 => "tab",
        313 => "caps",
        329 => "ins",
        330 => "home",
        331 => "pgup",
        332 => "del",
        333 => "end",
        334 => "pgdn",
        335 => "→",
        336 => "←",
        337 => "↓",
        338 => "↑",
        _ => "",
    };
    if !named.is_empty() {
        return named.to_string();
    }
    // Scancode letters (260=A‥285=Z) and digits (286=1‥294=9, 295=0).
    if (260..=285).contains(&c) {
        return ((b'a' + (c - 260) as u8) as char).to_string();
    }
    if (286..=294).contains(&c) {
        return ((b'1' + (c - 286) as u8) as char).to_string();
    }
    if c == 295 {
        return "0".to_string();
    }
    // Function keys F1–F12 (314‥325) and F13–F24 (360‥371).
    if (314..=325).contains(&c) {
        return format!("F{}", c - 313);
    }
    if (360..=371).contains(&c) {
        return format!("F{}", c - 347);
    }
    // Unknown / reserved code — surface the raw value rather than guess.
    format!("0x{:02X}", c)
}

/// Decode a device_chords `actions` BLOB (produced by serial.rs `compress_actions`)
/// back into human-readable key labels, returning one combo string per chord row.
///
/// Encoding: variable-length 8/13-bit values.
///   - If byte > 0 and byte < 32: 13-bit value = (byte << 8) | next_byte.
///   - Otherwise: 8-bit value = byte.
/// 0x00 is padding and skipped. Each code is mapped via [`action_label`].
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

    let mut labels: Vec<String> = codes.into_iter().map(action_label).collect();
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

/// Letters whose keys live on the left-thumb joystick cluster, and right-thumb
/// cluster. One thumb can only actuate one stick-direction at a time, so a single
/// chord may contain AT MOST ONE letter from each cluster — a constraint coarser
/// than the per-stick `action_to_group` map (which only blocks two keys on the
/// *same* stick). `dup` is not a word letter, so it's omitted.
const LEFT_THUMB: [char; 7] = ['m', 'c', 'k', 'v', 'g', 'z', 'w'];
const RIGHT_THUMB: [char; 7] = ['p', 'd', 'f', 'h', 'x', 'b', 'q'];

/// Which thumb cluster a letter belongs to: `Some(0)` left, `Some(1)` right,
/// `None` if the letter isn't on either thumb's sticks (no constraint).
fn thumb_side(c: char) -> Option<u8> {
    if LEFT_THUMB.contains(&c) {
        Some(0)
    } else if RIGHT_THUMB.contains(&c) {
        Some(1)
    } else {
        None
    }
}

/// Estimate the syllable count of an English word.
///
/// Vowel-group counting with a silent-trailing-`e` correction — the same core
/// heuristic popular syllable libraries (NLTK, the `syllable` crates) reduce to,
/// and it works on novel/non-dictionary words a lookup table would miss. Precise
/// enough to tell monosyllables (cat, leak, his, make) from polysyllables
/// (keyboard, water, table). Used only to gate compound-chord splits.
pub(super) fn estimate_syllables(word: &str) -> usize {
    let chars: Vec<char> = word
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if chars.is_empty() {
        return 0;
    }
    let mut count = 0usize;
    let mut prev_vowel = false;
    for &c in &chars {
        let is_vowel = VOWELS.contains(&c);
        if is_vowel && !prev_vowel {
            count += 1;
        }
        prev_vowel = is_vowel;
    }
    // Silent trailing 'e' (make, lake) collapses a vowel group, but "-le" after a
    // consonant keeps its syllable (table, candle), so leave that case alone.
    if count > 1 && chars.len() >= 2 {
        let last = chars[chars.len() - 1];
        let penult = chars[chars.len() - 2];
        if last == 'e' && penult != 'l' {
            count -= 1;
        }
    }
    count.max(1)
}

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
    action_mirror: &HashMap<u16, u16>,
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
        let mut used_thumbs: HashSet<u8> = HashSet::new();

        // Can `c` be added without a thumb or stick collision? Records usage on success.
        let try_place = |c: char,
                             result: &mut Vec<char>,
                             used_groups: &mut HashSet<usize>,
                             used_thumbs: &mut HashSet<u8>|
         -> bool {
            if result.contains(&c) {
                return false;
            }
            let side = thumb_side(c);
            if let Some(s) = side {
                if used_thumbs.contains(&s) {
                    return false;
                }
            }
            let group = action_to_group.get(&(c as u16)).copied();
            if let Some(g) = group {
                if used_groups.contains(&g) {
                    return false;
                }
            }
            if let Some(s) = side {
                used_thumbs.insert(s);
            }
            if let Some(g) = group {
                used_groups.insert(g);
            }
            result.push(c);
            true
        };

        for &c in candidates {
            if try_place(c, &mut result, &mut used_groups, &mut used_thumbs) {
                if result.len() >= max {
                    break;
                }
                continue;
            }
            // Blocked. For a thumb-cluster letter, the mirror-hand key at the same
            // direction is the natural alternative (e.g. p busy → v). Substitute it
            // rather than dropping the letter, when the mirror is free.
            if thumb_side(c).is_some() {
                if let Some(&mcode) = action_mirror.get(&(c as u16)) {
                    let mc = mcode as u8 as char;
                    if mc.is_ascii_alphabetic()
                        && try_place(mc, &mut result, &mut used_groups, &mut used_thumbs)
                    {
                        if result.len() >= max {
                            break;
                        }
                    }
                }
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

/// True if a single chord `part` (e.g. `"e + m + o + r"`) presses two keys that
/// share a joystick group — physically impossible on the device. Only single
/// printable-letter tokens are checked; named/multi-char tokens are ignored.
/// When the map is empty (no layout data) we can't judge, so we don't filter.
fn part_violates_joystick(part: &str, action_to_group: &HashMap<u16, usize>) -> bool {
    let mut used: HashSet<usize> = HashSet::new();
    let mut used_thumbs: HashSet<u8> = HashSet::new();
    for tok in part.split(" + ") {
        let mut chars = tok.trim().chars();
        if let (Some(ch), None) = (chars.next(), chars.next()) {
            // Thumb-cluster constraint applies regardless of layout data.
            if let Some(side) = thumb_side(ch) {
                if !used_thumbs.insert(side) {
                    return true;
                }
            }
            // Per-stick group constraint needs layout data; skip when absent.
            if !action_to_group.is_empty() {
                if let Some(&group) = action_to_group.get(&(ch as u16)) {
                    if !used.insert(group) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// True if a single chord `part` is ergonomically awkward: on one hand the middle
/// finger is pushed sideways (left/right) while BOTH the index and ring fingers
/// also hold their sticks, boxing the middle in with no room to splay. `finger_map`
/// is action_code → (hand, finger 0=index/1=middle/2=ring, horizontal). Empty map
/// (unknown layout) → never awkward.
fn part_is_awkward(part: &str, finger_map: &HashMap<u16, (u8, u8, bool)>) -> bool {
    if finger_map.is_empty() {
        return false;
    }
    let mut index_used = [false, false];
    let mut ring_used = [false, false];
    let mut middle_horizontal = [false, false];
    for tok in part.split(" + ") {
        let mut chars = tok.trim().chars();
        if let (Some(ch), None) = (chars.next(), chars.next()) {
            if let Some(&(hand, finger, horizontal)) = finger_map.get(&(ch as u16)) {
                let h = hand as usize;
                match finger {
                    0 => index_used[h] = true,
                    1 => {
                        if horizontal {
                            middle_horizontal[h] = true;
                        }
                    }
                    2 => ring_used[h] = true,
                    _ => {}
                }
            }
        }
    }
    (0..2).any(|h| middle_horizontal[h] && index_used[h] && ring_used[h])
}

/// Given an awkward single-chord `part`, drop the middle-finger letter(s) on the
/// hand(s) where the awkward triple occurs — the minimal edit that relieves the
/// crowding while keeping every other letter (e.g. `c + l + r + s + t` → `c + r +
/// s + t`, dropping the middle `l`). Returns the fixed combo string, or `None` if
/// the part isn't awkward or removing the letter(s) leaves nothing to drop.
fn deawkward(part: &str, finger_map: &HashMap<u16, (u8, u8, bool)>) -> Option<String> {
    if finger_map.is_empty() {
        return None;
    }
    let mut index_used = [false, false];
    let mut ring_used = [false, false];
    let mut middle_horizontal = [false, false];
    for tok in part.split(" + ") {
        let mut chars = tok.trim().chars();
        if let (Some(ch), None) = (chars.next(), chars.next()) {
            if let Some(&(hand, finger, horizontal)) = finger_map.get(&(ch as u16)) {
                let h = hand as usize;
                match finger {
                    0 => index_used[h] = true,
                    1 => {
                        if horizontal {
                            middle_horizontal[h] = true;
                        }
                    }
                    2 => ring_used[h] = true,
                    _ => {}
                }
            }
        }
    }
    let awkward_hand =
        |h: usize| middle_horizontal[h] && index_used[h] && ring_used[h];
    if !awkward_hand(0) && !awkward_hand(1) {
        return None;
    }
    let kept: Vec<String> = part
        .split(" + ")
        .filter(|tok| {
            let mut chars = tok.trim().chars();
            if let (Some(ch), None) = (chars.next(), chars.next()) {
                if let Some(&(hand, finger, horizontal)) = finger_map.get(&(ch as u16)) {
                    // Drop a middle-finger horizontal key on an awkward hand.
                    if finger == 1 && horizontal && awkward_hand(hand as usize) {
                        return false;
                    }
                }
            }
            true
        })
        .map(|t| t.trim().to_string())
        .collect();
    if kept.len() < 2 {
        return None;
    }
    let mut sorted = kept;
    sorted.sort();
    Some(sorted.join(" + "))
}

/// Search subsets of `pool` for the smallest chord (2-6 keys) that is physically
/// pressable and NOT already occupied by a device chord. Subsets are tried
/// smallest-first, low-index-first, so word letters (placed at the front of the
/// pool) are preferred over mirror-hand alternates. Returns the combo string, or
/// `None` only when every subset is occupied or unpressable. This is the backstop
/// that guarantees the overlay a conflict-free, non-swap option whenever one exists.
fn first_free_combo(
    pool: &[char],
    action_to_group: &HashMap<u16, usize>,
    action_finger: &HashMap<u16, (u8, u8, bool)>,
    combo_to_phrases: &HashMap<String, Vec<String>>,
    seen_labels: &HashSet<String>,
) -> Option<String> {
    let n = pool.len().min(12);
    // Pass 1 rejects awkward combos; pass 2 accepts them as a last resort, so an
    // ergonomic option always wins when one exists but a word is never left empty.
    for allow_awkward in [false, true] {
        for size in 2..=6usize {
            if size > n {
                break;
            }
            for mask in 1u32..(1u32 << n) {
                if mask.count_ones() as usize != size {
                    continue;
                }
                let mut labels: Vec<String> = (0..n)
                    .filter(|i| mask & (1 << i) != 0)
                    .map(|i| pool[i].to_string())
                    .collect();
                labels.sort();
                let s = labels.join(" + ");
                if seen_labels.contains(&s) {
                    continue;
                }
                if part_violates_joystick(&s, action_to_group) {
                    continue;
                }
                if combo_to_phrases.contains_key(&s) {
                    continue; // occupied by an existing device chord
                }
                if !allow_awkward && part_is_awkward(&s, action_finger) {
                    continue;
                }
                return Some(s);
            }
        }
    }
    None
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
    action_mirror: &HashMap<u16, u16>,
    action_finger: &HashMap<u16, (u8, u8, bool)>,
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
    let primary_chars = suggest_chord_combo(phrase, action_to_group, action_mirror);
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
            let cons_chars = suggest_chord_combo(&cons_phrase, action_to_group, action_mirror);
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
            make_combo_string(suggest_chord_combo(stem, action_to_group, action_mirror))
        });
        let affix_combo = make_combo_string(suggest_chord_combo(affix, action_to_group, action_mirror));
        if stem_combo.is_empty() || affix_combo.is_empty() {
            return None;
        }
        Some(ChordCombo {
            kind: "compound".to_string(),
            parts: vec![stem_combo, affix_combo],
            conflicts: vec![],
        })
    };

    // Compound chords split a word into sequential parts — only sensible for
    // multi-syllable words. Suppress all compound generation (sections 2-4) for
    // monosyllables (his, leak, cat) so they only ever get a single chord.
    let allow_compound = estimate_syllables(phrase) >= 2;

    // --- 2. Device chord prefix match (exact + stem transforms) — highest priority ---
    // Run before generic suffix/prefix heuristics: an existing chord as the stem
    // is a better suggestion than a raw letter-derived split.
    // Sort by length descending so "keyboard" beats "key" for "keyboardist".
    let mut chord_entries: Vec<(&String, &String)> = phrase_to_combo.iter().collect();
    chord_entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    for (existing_phrase, existing_combo) in &chord_entries {
        if !allow_compound { break; }
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
                        make_combo_string(suggest_chord_combo(tail, action_to_group, action_mirror));
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
        if !allow_compound { break; }
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
        if !allow_compound { break; }
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

    // Fail-safe: drop any combo whose single-chord part collides on a joystick,
    // and any compound with more than 2 parts. The generated paths
    // (primary/consonant/affix) already respect the joystick map, but
    // device-chord-derived parts (compound prefix matches) are spliced in
    // unvalidated — this catches every path uniformly so no same-joystick
    // suggestion (e.g. r + e on the M4G) can resurface. 3+ part compounds are
    // never worth the cognitive load: a 2-part split is the practical ceiling.
    results.retain(|c| {
        c.parts.len() <= 2
            && !c
                .parts
                .iter()
                .any(|p| part_violates_joystick(p, action_to_group))
    });

    // Conflict-free guarantee: if no surviving option is a clean single chord
    // (single part, conflict-free, ergonomic), search the word's own letters —
    // plus mirror-hand alternates for thumb-cluster letters — for a free,
    // pressable, non-awkward combo. Covers both the all-conflicting case and the
    // case where the best single chord is awkward (e.g. "realistic" → drop the
    // middle-finger letter), so the overlay favors a clean single over a compound.
    let has_clean_single = results.iter().any(|c| {
        c.kind == "chord"
            && c.parts.len() == 1
            && c.conflicts.is_empty()
            && !c.parts.iter().any(|p| part_is_awkward(p, action_finger))
    });
    if !has_clean_single {
        // First choice: recover a clean chord by dropping just the offending
        // middle-finger letter from an awkward single chord — keeps it mnemonic.
        let mut injected = false;
        let awkward_singles: Vec<String> = results
            .iter()
            .filter(|c| c.kind == "chord" && c.parts.len() == 1 && c.conflicts.is_empty())
            .map(|c| c.parts[0].clone())
            .filter(|p| part_is_awkward(p, action_finger))
            .collect();
        for aw in awkward_singles {
            if let Some(fixed) = deawkward(&aw, action_finger) {
                if !seen_labels.contains(&fixed)
                    && !part_violates_joystick(&fixed, action_to_group)
                    && !combo_to_phrases.contains_key(&fixed)
                    && !part_is_awkward(&fixed, action_finger)
                {
                    seen_labels.insert(fixed.clone());
                    results.push(ChordCombo {
                        kind: "chord".to_string(),
                        parts: vec![fixed],
                        conflicts: vec![],
                    });
                    injected = true;
                    break;
                }
            }
        }

        // Backstop: nothing recoverable that way (e.g. every heuristic combo is
        // occupied). Search the word's letters + thumb mirrors for any free combo.
        if !injected {
            let mut pool: Vec<char> = Vec::new();
            let mut pool_seen: HashSet<char> = HashSet::new();
            for c in phrase
                .chars()
                .filter(|c| c.is_ascii_alphabetic())
                .map(|c| c.to_ascii_lowercase())
            {
                if pool_seen.insert(c) {
                    pool.push(c);
                }
            }
            let mirrors: Vec<char> = pool
                .iter()
                .filter(|c| thumb_side(**c).is_some())
                .filter_map(|c| action_mirror.get(&(*c as u16)))
                .map(|&m| m as u8 as char)
                .filter(|m| m.is_ascii_alphabetic())
                .collect();
            for m in mirrors {
                if pool_seen.insert(m) {
                    pool.push(m);
                }
            }
            if let Some(free) = first_free_combo(
                &pool,
                action_to_group,
                action_finger,
                combo_to_phrases,
                &seen_labels,
            ) {
                results.push(ChordCombo {
                    kind: "chord".to_string(),
                    parts: vec![free],
                    conflicts: vec![],
                });
            }
        }
    }

    // Sort by descending score so the best option is always first (and becomes
    // primary in the overlay). Criteria in priority order:
    //   1. Conflict-free > conflicting    (-1000 penalty per conflict)
    //   2. Ergonomic > awkward            (-200 if a part is awkward; still beats a swap)
    //   3. Single chord > compound        (+50 for chord kind)
    //   4. Fewer total keys               (-10 per key across all parts)
    //   5. Fewer compound parts           (-30 per extra part beyond the first)
    results.sort_by_key(|c| {
        let conflict_penalty = if c.conflicts.is_empty() { 0i32 } else { -1000 };
        let awkward_penalty = if c.parts.iter().any(|p| part_is_awkward(p, action_finger)) {
            -200
        } else {
            0
        };
        let kind_bonus = if c.kind == "chord" { 50i32 } else { 0 };
        let total_keys: i32 = c.parts.iter().map(|p| p.split(" + ").count() as i32).sum();
        let part_penalty = (c.parts.len() as i32 - 1) * 30;
        std::cmp::Reverse(
            conflict_penalty + awkward_penalty + kind_bonus - total_keys * 10 - part_penalty,
        )
    });

    results
}

#[cfg(test)]
mod tests {
    use super::{
        action_label, deawkward, decode_actions_blob, estimate_syllables, generate_combos,
        part_is_awkward, part_violates_joystick,
    };
    use std::collections::HashMap;

    // Right-hand finger map for the t/s (index/ring, down) + l/j (middle, horizontal)
    // cluster, matching the real M4G layout used in the awkwardness examples.
    fn rh_finger_map() -> HashMap<u16, (u8, u8, bool)> {
        [
            (b't' as u16, (1u8, 0u8, false)),
            (b's' as u16, (1, 2, false)),
            (b'l' as u16, (1, 1, true)),
            (b'j' as u16, (1, 1, true)),
            (b'n' as u16, (1, 1, false)),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn deawkward_drops_only_the_middle_letter() {
        let fm = rh_finger_map();
        // c,r neutral (not in map) → kept; l is the offending middle key → dropped.
        assert_eq!(
            deawkward("c + l + r + s + t", &fm).as_deref(),
            Some("c + r + s + t")
        );
        // Not awkward → no change.
        assert_eq!(deawkward("r + s + t", &fm), None);
    }

    #[test]
    fn awkward_primary_recovers_a_clean_single() {
        // All-consonant "word" whose primary chord is the awkward l+s+t core.
        // Generation must surface a clean (non-awkward) single chord, and it must
        // outrank the awkward one.
        let fm = rh_finger_map();
        let combos = generate_combos(
            "rlstc",
            &HashMap::new(),
            &HashMap::new(),
            &fm,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(
            combos
                .iter()
                .any(|c| c.parts.len() == 1 && !part_is_awkward(&c.parts[0], &fm)),
            "expected a clean single chord in {combos:?}"
        );
        // Best-ranked option must not be awkward.
        let top = &combos[0];
        assert!(
            !top.parts.iter().any(|p| part_is_awkward(p, &fm)),
            "top option should be ergonomic: {top:?}"
        );
    }

    #[test]
    fn syllable_counts_monosyllables() {
        for w in ["his", "leak", "cat", "make", "the", "stream", "though"] {
            assert_eq!(estimate_syllables(w), 1, "{w} should be 1 syllable");
        }
    }

    #[test]
    fn syllable_counts_polysyllables() {
        assert_eq!(estimate_syllables("keyboard"), 2);
        assert_eq!(estimate_syllables("water"), 2);
        assert_eq!(estimate_syllables("table"), 2); // -le keeps its syllable
        assert!(estimate_syllables("category") >= 3);
    }

    #[test]
    fn monosyllable_never_yields_compound() {
        // Device chord "lea" exists; without the syllable gate "leak" could split
        // into a compound. One syllable → single chords only.
        let action_to_group: HashMap<u16, usize> = HashMap::new();
        let combo_to_phrases: HashMap<String, Vec<String>> = HashMap::new();
        let mut phrase_to_combo: HashMap<String, String> = HashMap::new();
        phrase_to_combo.insert("lea".to_string(), "e + l".to_string());

        let combos = generate_combos(
            "leak",
            &action_to_group,
            &HashMap::new(),
            &HashMap::new(),
            &combo_to_phrases,
            &phrase_to_combo,
        );
        assert!(!combos.is_empty());
        assert!(
            combos.iter().all(|c| c.kind == "chord"),
            "monosyllable must not produce compound combos: {combos:?}"
        );
    }

    #[test]
    fn thumb_cluster_collision_detected_without_layout() {
        // No layout map, but thumb constraint still applies.
        let map: HashMap<u16, usize> = HashMap::new();
        // m and c both on the left thumb → impossible.
        assert!(part_violates_joystick("c + m", &map));
        // p and d both on the right thumb → impossible.
        assert!(part_violates_joystick("d + p", &map));
        // One per side + neutral vowels → fine.
        assert!(!part_violates_joystick("m + o + p", &map));
        assert!(!part_violates_joystick("a + e + o", &map));
    }

    #[test]
    fn generated_chord_respects_thumb_clusters() {
        let empty: HashMap<u16, usize> = HashMap::new();
        // "mock" = m,o,c,k — m/c/k all left thumb, so at most one can appear.
        let combos = generate_combos(
            "mock",
            &empty,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        for c in &combos {
            for part in &c.parts {
                let lefts = part
                    .split(" + ")
                    .filter(|t| matches!(*t, "m" | "c" | "k" | "v" | "g" | "z" | "w"))
                    .count();
                assert!(lefts <= 1, "part {part:?} uses >1 left-thumb letter");
            }
        }
    }

    #[test]
    fn mirror_substitutes_blocked_thumb_letter() {
        // "drop" = d,r,o,p — d and p are both right-thumb, so they can't co-press.
        // With p↔v in the mirror map, p should be replaced by v rather than dropped.
        let action_to_group: HashMap<u16, usize> = HashMap::new();
        let mirror: HashMap<u16, u16> =
            [(b'p' as u16, b'v' as u16), (b'v' as u16, b'p' as u16)]
                .into_iter()
                .collect();
        let combos = generate_combos(
            "drop",
            &action_to_group,
            &mirror,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        // No surviving part may contain a bare "p" (the blocked key)…
        for c in &combos {
            for part in &c.parts {
                assert!(
                    !part.split(" + ").any(|t| t == "p"),
                    "blocked key p should have been mirrored: {part:?}"
                );
            }
        }
        // …and the mirror "v" should appear in at least one option.
        assert!(
            combos
                .iter()
                .any(|c| c.parts.iter().any(|p| p.split(" + ").any(|t| t == "v"))),
            "expected a mirrored v in {combos:?}"
        );
    }

    #[test]
    fn always_offers_a_conflict_free_option_when_one_exists() {
        // "race" = r,a,c,e. Occupy the two combos the heuristics would produce
        // (primary all-letters and consonant-only) so every heuristic option
        // conflicts — the subset backstop must still surface a free combo.
        let action_to_group: HashMap<u16, usize> = HashMap::new();
        let mut combo_to_phrases: HashMap<String, Vec<String>> = HashMap::new();
        combo_to_phrases.insert("a + c + e + r".to_string(), vec!["occupied".to_string()]);
        combo_to_phrases.insert("c + r".to_string(), vec!["occupied".to_string()]);

        let combos = generate_combos(
            "race",
            &action_to_group,
            &HashMap::new(),
            &HashMap::new(),
            &combo_to_phrases,
            &HashMap::new(),
        );
        assert!(
            combos.iter().any(|c| c.conflicts.is_empty()),
            "must always surface a conflict-free option: {combos:?}"
        );
    }

    #[test]
    fn awkward_detects_middle_horizontal_with_both_neighbors() {
        // finger_map: hand 1 (right). index=t, ring=s (down), middle l=left, j=right.
        let fm: HashMap<u16, (u8, u8, bool)> = [
            (b't' as u16, (1u8, 0u8, false)), // index, down
            (b's' as u16, (1, 2, false)),     // ring, down
            (b'l' as u16, (1, 1, true)),      // middle, horizontal (left)
            (b'j' as u16, (1, 1, true)),      // middle, horizontal (right)
            (b'n' as u16, (1, 1, false)),     // middle, vertical (down)
        ]
        .into_iter()
        .collect();

        // Middle horizontal + both neighbors → awkward.
        assert!(part_is_awkward("l + s + t", &fm));
        assert!(part_is_awkward("j + s + t", &fm));
        // Only one neighbor → fine.
        assert!(!part_is_awkward("l + t", &fm));
        assert!(!part_is_awkward("l + s", &fm));
        // Middle vertical with both neighbors → fine.
        assert!(!part_is_awkward("n + s + t", &fm));
        // Empty finger map (unknown layout) → never awkward.
        assert!(!part_is_awkward("l + s + t", &HashMap::new()));
    }

    #[test]
    fn joystick_collision_detected_within_a_part() {
        // r (114) and e (101) both in group 2 — can't be pressed together.
        let map: HashMap<u16, usize> =
            [(114u16, 2usize), (101, 2), (109, 0), (111, 1)].into_iter().collect();
        assert!(part_violates_joystick("e + m + o + r", &map));
        assert!(!part_violates_joystick("m + o + r", &map)); // only one of r/e
        assert!(!part_violates_joystick("m + o", &map));
    }

    #[test]
    fn joystick_empty_map_never_filters() {
        let map: HashMap<u16, usize> = HashMap::new();
        assert!(!part_violates_joystick("e + m + o + r", &map));
    }

    #[test]
    fn labels_printable_ascii_as_char() {
        assert_eq!(action_label(b'a' as u16), "a");
        assert_eq!(action_label(b'Z' as u16), "Z");
        assert_eq!(action_label(b'+' as u16), "+");
    }

    #[test]
    fn labels_modifiers_and_cc_actions() {
        assert_eq!(action_label(512), "ctrl"); // LEFT_CTRL
        assert_eq!(action_label(517), "shift"); // RIGHT_SHIFT
        assert_eq!(action_label(515), "cmd"); // LEFT_GUI
        assert_eq!(action_label(536), "dup"); // DUP (was 0x218)
        assert_eq!(action_label(544), "space"); // SPACERIGHT
        assert_eq!(action_label(550), "km2"); // numeric layer
    }

    #[test]
    fn labels_named_keys_letters_and_fkeys() {
        assert_eq!(action_label(296), "enter");
        assert_eq!(action_label(335), "→");
        assert_eq!(action_label(260), "a"); // KEY_A scancode
        assert_eq!(action_label(285), "z"); // KEY_Z
        assert_eq!(action_label(295), "0"); // KEY_0
        assert_eq!(action_label(314), "F1");
        assert_eq!(action_label(325), "F12");
        assert_eq!(action_label(360), "F13");
    }

    #[test]
    fn unknown_code_falls_back_to_hex() {
        assert_eq!(action_label(580), "0x244"); // reserved / not in public keymap
    }

    #[test]
    fn decodes_13bit_and_8bit_codes() {
        // 0x61 'a' (8-bit) + 0x0244 (13-bit: lead 0x02 < 32, then 0x44).
        // Sorted: "0x244" < "a".
        assert_eq!(decode_actions_blob(&[0x61, 0x02, 0x44]), "0x244 + a");
        // Padding 0x00 is skipped.
        assert_eq!(decode_actions_blob(&[0x00, 0x62]), "b");
    }
}
