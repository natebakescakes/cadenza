// SQLite persistence layer (rusqlite, plain `bundled`).
//
// DB path (macOS): `~/Library/Application Support/Cadenza/cadenza.sqlite3`.
//
// Encryption path chosen: plain bundled SQLite + Argon2 password hash stored in
// a `meta` table. This keeps the build green (no OpenSSL/SQLCipher) while still
// implementing the dbInit/dbUnlock UX (set/verify password). Real at-rest
// encryption (SQLCipher PRAGMA key) can be layered in later without changing
// the public API of this module.

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rusqlite::{params, Connection, OptionalExtension};

use crate::types::{
    ActivityBlock, BanlistEntry, ChordRecord, Proficiency, Suggestion, WordRecord, WpmSample,
    WpmSummary,
};

/// Estimated time a chord takes to fire on-device (ms). Used for suggestion savings.
const ESTIMATED_CHORD_MS: f64 = 150.0;

/// Gaps longer than this between consecutive units are treated as idle/thinking
/// time and capped, so they don't deflate the active-typing denominator.
const IDLE_CAP_MS: i64 = 5000;

/// A gap larger than this between units starts a new "session" for the session WPM number.
const SESSION_GAP_MS: i64 = 30_000;

/// A single logged unit pulled from `wpm_samples` for query-time WPM computation.
struct RawUnit {
    t: i64,
    chars: i64,
    source: String,
}

/// Active-typing milliseconds across an ordered slice of units: sum of capped
/// gaps to the previous unit, with a floor to avoid divide-by-zero / spikes.
fn active_ms(units: &[RawUnit]) -> f64 {
    let mut total: i64 = 0;
    for w in units.windows(2) {
        let gap = (w[1].t - w[0].t).max(0);
        total += gap.min(IDLE_CAP_MS);
    }
    (total as f64).max(1000.0)
}

/// chars/5 over active minutes for the given units.
fn wpm_of(units: &[RawUnit]) -> f64 {
    if units.is_empty() {
        return 0.0;
    }
    let chars: i64 = units.iter().map(|u| u.chars).sum();
    let words = chars as f64 / 5.0;
    words / (active_ms(units) / 60000.0)
}

/// Which suffix class matched — used to gate Layer B.
#[derive(PartialEq)]
enum SuffixKind {
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
fn candidate_bases(word: &str) -> (Vec<String>, SuffixKind) {
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
fn is_inflected(phrase: &str, known_bases: &std::collections::HashSet<String>) -> bool {
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
fn decode_actions_blob(blob: &[u8]) -> String {
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

/// Owns the open SQLite connection.
pub struct Storage {
    conn: Connection,
}

impl Storage {
    // --- Path helpers -----------------------------------------------------

    /// App data directory: `~/Library/Application Support/Cadenza/`.
    pub fn data_dir() -> PathBuf {
        let mut p = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("Cadenza");
        p
    }

    /// Full path to the sqlite file.
    pub fn db_path() -> PathBuf {
        let mut p = Self::data_dir();
        p.push("cadenza.sqlite3");
        p
    }

    /// Whether a Cadenza DB file already exists on disk.
    pub fn is_initialized() -> bool {
        Self::db_path().exists()
    }

    // --- Open / schema ----------------------------------------------------

    /// Open (or create) a connection to the DB file at the standard path, with
    /// WAL enabled and the schema created. Used by both init/unlock and by the
    /// detector thread (which opens its own independent connection).
    pub fn open() -> Result<Connection> {
        let dir = Self::data_dir();
        std::fs::create_dir_all(&dir)?;
        let conn = Connection::open(Self::db_path())?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::create_schema(&conn)?;
        Ok(conn)
    }

    /// Wrap an already-open connection (used by the detector thread, which
    /// opens its own Connection to the same WAL file).
    pub fn from_connection(conn: Connection) -> Self {
        Self { conn }
    }

    /// Generic single-value i64 lookup; returns 0 if absent/error.
    pub fn scalar_i64(&self, sql: &str, key: &str) -> i64 {
        self.conn
            .query_row(sql, params![key], |r| r.get::<_, i64>(0))
            .optional()
            .ok()
            .flatten()
            .unwrap_or(0)
    }

    /// Pull all logged units (t, chars, source) in time order, for query-time WPM.
    /// `since` filters to units with t >= since (use 0 for all).
    fn raw_units(&self, since: i64) -> Vec<RawUnit> {
        let mut out = Vec::new();
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT t, chars, source FROM wpm_samples
             WHERE t >= ?1 AND source IN ('manual','chorded') AND chars > 0
             ORDER BY t ASC",
        ) {
            let rows = stmt.query_map(params![since], |r| {
                Ok(RawUnit {
                    t: r.get(0)?,
                    chars: r.get(1)?,
                    source: r.get(2)?,
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

    /// Live rolling WPM over the trailing 60s wall-clock window. Used by the
    /// detector to populate the live `wpm` event.
    pub fn rolling_wpm(&self, now: i64) -> f64 {
        let units = self.raw_units(now - 60_000);
        wpm_of(&units)
    }

    fn create_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS words (
                word TEXT PRIMARY KEY,
                frequency INTEGER NOT NULL DEFAULT 0,
                last_used INTEGER NOT NULL DEFAULT 0,
                total_time_ms INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS chords (
                phrase TEXT PRIMARY KEY,
                frequency INTEGER NOT NULL DEFAULT 0,
                last_used INTEGER NOT NULL DEFAULT 0,
                total_time_ms INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS chord_manual (
                phrase TEXT PRIMARY KEY,
                manual_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS sessions (
                id INTEGER PRIMARY KEY,
                start INTEGER NOT NULL,
                end INTEGER NOT NULL,
                char_count INTEGER NOT NULL DEFAULT 0,
                word_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS wpm_samples (
                t INTEGER NOT NULL,
                wpm REAL NOT NULL,
                source TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS device_chords (
                phrase TEXT NOT NULL,
                actions BLOB,
                device_id TEXT
            );
            CREATE TABLE IF NOT EXISTS banlist (
                word TEXT PRIMARY KEY,
                added INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_wpm_t ON wpm_samples(t);
            CREATE INDEX IF NOT EXISTS idx_device_chords_phrase ON device_chords(phrase);
            ",
        )?;
        // Migration: store the raw character count of each logged unit so WPM can
        // be computed at query time from (t, chars, source). Idempotent: ignore
        // the "duplicate column" error if it already exists.
        let _ = conn.execute(
            "ALTER TABLE wpm_samples ADD COLUMN chars INTEGER DEFAULT 0",
            [],
        );
        // Migration: separate chord_errors table for "fired then deleted" signals.
        // Idempotent: IF NOT EXISTS guards the CREATE.
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS chord_errors (
                phrase TEXT PRIMARY KEY,
                error_count INTEGER NOT NULL DEFAULT 0,
                last_error INTEGER NOT NULL DEFAULT 0
            );",
        );
        // Migration: per-word clean occurrence count for accuracy rate.
        // Idempotent: ignore duplicate-column error.
        let _ = conn.execute(
            "ALTER TABLE words ADD COLUMN clean_count INTEGER DEFAULT 0",
            [],
        );
        // Migration: hidden_words for display filtering (hide ≠ ban; logged data kept).
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS hidden_words (
                word TEXT PRIMARY KEY,
                added INTEGER NOT NULL DEFAULT 0
            );",
        );
        Ok(())
    }

    // --- Init / unlock (password via Argon2 hash in meta) -----------------

    /// Create the DB + schema and store the Argon2 hash of `password`.
    pub fn init(password: &str) -> Result<Self> {
        let conn = Self::open()?;
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow!("hash error: {e}"))?
            .to_string();
        conn.execute(
            "INSERT INTO meta(key, value) VALUES('password_hash', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![hash],
        )?;
        Ok(Self { conn })
    }

    /// Open (or create) the DB without verifying any password — dev bypass only.
    /// Schema is created/migrated as usual; no password hash is written or read.
    pub fn open_no_auth() -> Result<Self> {
        let conn = Self::open()?;
        Ok(Self { conn })
    }

    /// Open an existing DB and verify `password`. Errors if the password is wrong.
    pub fn unlock(password: &str) -> Result<Self> {
        let conn = Self::open()?;
        let stored: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'password_hash'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let stored = stored.ok_or_else(|| anyhow!("database not initialized"))?;
        let parsed = PasswordHash::new(&stored).map_err(|e| anyhow!("bad hash: {e}"))?;
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .map_err(|_| anyhow!("incorrect password"))?;
        Ok(Self { conn })
    }

    // --- Write paths (used by the detector thread) ------------------------

    /// Increment a word's frequency, bump last_used, accumulate typing time.
    /// `clean` = true when the occurrence had zero backspace corrections.
    pub fn log_word(&self, word: &str, ts_ms: i64, time_ms: i64, clean: bool) -> Result<()> {
        let clean_inc: i64 = if clean { 1 } else { 0 };
        self.conn.execute(
            "INSERT INTO words(word, frequency, last_used, total_time_ms, clean_count)
             VALUES(?1, 1, ?2, ?3, ?4)
             ON CONFLICT(word) DO UPDATE SET
                frequency     = frequency + 1,
                last_used     = ?2,
                total_time_ms = total_time_ms + ?3,
                clean_count   = clean_count + ?4",
            params![word, ts_ms, time_ms, clean_inc],
        )?;
        Ok(())
    }

    /// Increment a chord phrase's frequency, bump last_used, accumulate time.
    pub fn log_chord(&self, phrase: &str, ts_ms: i64, time_ms: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO chords(phrase, frequency, last_used, total_time_ms)
             VALUES(?1, 1, ?2, ?3)
             ON CONFLICT(phrase) DO UPDATE SET
                frequency = frequency + 1,
                last_used = ?2,
                total_time_ms = total_time_ms + ?3",
            params![phrase, ts_ms, time_ms],
        )?;
        Ok(())
    }

    /// Bump the manual-typing counter for a phrase (used for proficiency usage rate).
    pub fn bump_chord_manual(&self, phrase: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO chord_manual(phrase, manual_count) VALUES(?1, 1)
             ON CONFLICT(phrase) DO UPDATE SET manual_count = manual_count + 1",
            params![phrase],
        )?;
        Ok(())
    }

    /// Record one chord-error event: the chord output was emitted then deleted
    /// before the buffer flushed (botched chord / Quickfix). Used for
    /// needs-practice ranking in proficiency.
    pub fn bump_chord_error(&self, phrase: &str, ts: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO chord_errors(phrase, error_count, last_error) VALUES(?1, 1, ?2)
             ON CONFLICT(phrase) DO UPDATE SET
               error_count = error_count + 1,
               last_error  = ?2",
            params![phrase, ts],
        )?;
        Ok(())
    }

    /// Record a logged unit: its flush timestamp, character count, and source
    /// ("manual" | "chorded"). WPM is computed at query time from these raw rows,
    /// so the legacy `wpm` column is left at 0.
    pub fn add_wpm_sample(&self, t: i64, chars: i64, source: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO wpm_samples(t, wpm, source, chars) VALUES(?1, 0, ?2, ?3)",
            params![t, source, chars],
        )?;
        Ok(())
    }

    /// Insert or update a session row.
    pub fn upsert_session(
        &self,
        id: i64,
        start: i64,
        end: i64,
        char_count: i64,
        word_count: i64,
    ) -> Result<i64> {
        if id <= 0 {
            self.conn.execute(
                "INSERT INTO sessions(start, end, char_count, word_count) VALUES(?1, ?2, ?3, ?4)",
                params![start, end, char_count, word_count],
            )?;
            Ok(self.conn.last_insert_rowid())
        } else {
            self.conn.execute(
                "UPDATE sessions SET start = ?2, end = ?3, char_count = ?4, word_count = ?5 WHERE id = ?1",
                params![id, start, end, char_count, word_count],
            )?;
            Ok(id)
        }
    }

    /// Whether a word is banned.
    pub fn is_banned(&self, word: &str) -> bool {
        self.conn
            .query_row("SELECT 1 FROM banlist WHERE word = ?1", params![word], |_| {
                Ok(())
            })
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    // --- Query paths (used by Tauri commands on the main thread) ----------

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
            "SELECT phrase, frequency, last_used, total_time_ms FROM chords
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

        // --- Chords per 5-min bucket (from chords.last_used) -----------------
        let mut bucket_chorded: std::collections::HashMap<i64, Vec<String>> =
            std::collections::HashMap::new();
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT phrase, last_used FROM chords WHERE last_used >= ?1 ORDER BY last_used ASC",
        ) {
            let rows = stmt.query_map(params![since], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            });
            if let Ok(rows) = rows {
                for item in rows.flatten() {
                    let key = (item.1 / BLOCK_MS) * BLOCK_MS;
                    bucket_chorded.entry(key).or_default().push(item.0);
                }
            }
        }

        // --- Merge into sorted blocks (all buckets that have any data) --------
        let mut all_keys: std::collections::HashSet<i64> = std::collections::HashSet::new();
        for k in bucket_wpm.keys() { all_keys.insert(*k); }
        for k in bucket_manual.keys() { all_keys.insert(*k); }
        for k in bucket_chorded.keys() { all_keys.insert(*k); }

        let mut blocks: Vec<ActivityBlock> = all_keys
            .into_iter()
            .map(|t| ActivityBlock {
                t,
                wpm: bucket_wpm.get(&t).copied().unwrap_or(0.0),
                manual_words: bucket_manual.remove(&t).unwrap_or_default(),
                chorded_words: bucket_chorded.remove(&t).unwrap_or_default(),
            })
            .collect();
        blocks.sort_by_key(|b| std::cmp::Reverse(b.t));
        blocks
    }

    /// Frequent words (len>=2) NOT already a device chord, ordered by score.
    pub fn suggestions(&self, limit: i64) -> Vec<Suggestion> {
        let lim = if limit <= 0 { 50 } else { limit };

        // --- 1. Fetch a generous over-set so the inflection post-filter still
        //        leaves enough results after pruning. ---
        let fetch_lim = lim * 4;
        let mut candidates: Vec<Suggestion> = Vec::new();
        if let Ok(mut stmt) = self.conn.prepare(
            // Clean words only: must contain a letter, and may ONLY contain
            // letters, apostrophes, and hyphens. NOTE: SQLite GLOB '*' is a
            // wildcard (any chars), not a regex quantifier — so we reject via
            // NOT GLOB on a negated char class rather than a positive pattern.
            // Match chord library case-insensitively via LOWER().
            "SELECT word, frequency, total_time_ms FROM words
             WHERE LENGTH(word) >= 2
               AND frequency >= 1
               AND LOWER(word) NOT IN (SELECT LOWER(phrase) FROM device_chords)
               AND LOWER(word) NOT IN (SELECT word FROM hidden_words)
               AND word GLOB '*[a-zA-Z]*'
               AND word NOT GLOB '*[^a-zA-Z''-]*'
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
                Ok(Suggestion {
                    phrase,
                    frequency,
                    score,
                    avg_manual_ms,
                    projected_saving_ms,
                })
            });
            if let Ok(rows) = rows {
                for r in rows.flatten() {
                    candidates.push(r);
                }
            }
        }

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
        //
        // This is a lightweight English stemmer heuristic: we inverse-inflect
        // each candidate to plausible root forms and exclude the candidate when
        // the root looks known (Layer A) or the candidate clearly carries a
        // grammatical suffix (Layer B, with length guards).
        //
        // PURPOSE: On CharaChorder, device special-keys append inflections
        // (-ing, -s, -ed, …) to a chorded base word, so suggesting "running"
        // is redundant if the user chords "run".  We want to surface the root.
        //
        // LAYER A (base exists in corpus/chordmap — high precision):
        //   Generate candidate root forms via inverse-inflection rules.
        //   If ANY generated root (different from the candidate itself) is in
        //   known_bases, exclude the candidate.
        //
        // LAYER B (suffix heuristic — covers bases not yet typed):
        //   If the candidate clearly has an inflectional suffix AND stripping it
        //   yields a base of ≥ 3 letters AND the candidate length is ≥ 5,
        //   exclude it even without a corpus hit.
        //   Length guards prevent false positives on short words (sing→s, bus,
        //   ring, red, etc.).
        //
        // FALSE POSITIVES are acceptable — suggestions are advisory, the user
        // can always chord or hide any word.  Irregular past tense (run→ran,
        // go→went) is NOT caught by suffix rules and will remain in suggestions.
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
                COALESCE(e.error_count, 0)
             FROM device_chords dc
             LEFT JOIN chords c        ON LOWER(c.phrase) = LOWER(dc.phrase)
             LEFT JOIN chord_manual m  ON LOWER(m.phrase) = LOWER(dc.phrase)
             LEFT JOIN chord_errors e  ON LOWER(e.phrase) = LOWER(dc.phrase)
             -- Only include chords the user has actually touched (fired OR errored).
             WHERE COALESCE(c.frequency, 0) + COALESCE(e.error_count, 0) >= 1",
        ) {
            let rows = stmt.query_map([], |r| {
                let phrase: String = r.get(0)?;
                let fired: i64 = r.get(1)?;
                let total: i64 = r.get(2)?;
                let manual: i64 = r.get(3)?;
                let errors: i64 = r.get(4)?;

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
                // Mastered: fired reliably, rarely deleted, AND actually preferred
                // over manual typing (usage_rate >= 0.7 = fires ≥70% of the time
                // the phrase appears).  Without usage_rate, frequently typed but
                // always-manual words (of, you, there) scored as mastered.
                let mastered = fired >= 3
                    && error_rate <= 0.1
                    && usage_rate >= 0.7;

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
        // Mastered chords are excluded by the UI (!mastered filter); this ordering
        // applies across the whole set so both groups are consistently ranked.
        //
        // practice_score:
        //   error_count * 5.0       — fired-then-deleted: strongest difficulty signal
        //   avg_fire_ms / 200.0     — slow fires indicate incomplete muscle memory
        //   (1 - usage_rate) * 2.0  — hand-typed instead of chorded: not habituated
        //
        // Tiebreak: total frequency (fired + manual) DESC so among equally-difficult
        // chords the higher-impact ones appear first.
        out.sort_by(|a, b| {
            let score = |p: &Proficiency| -> f64 {
                p.error_count as f64 * 5.0
                    + p.avg_fire_ms / 200.0
                    + (1.0 - p.usage_rate) * 2.0
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

    // --- Banlist ----------------------------------------------------------

    pub fn list_banlist(&self) -> Vec<BanlistEntry> {
        let mut out = Vec::new();
        if let Ok(mut stmt) = self
            .conn
            .prepare("SELECT word, added FROM banlist ORDER BY added DESC")
        {
            let rows = stmt.query_map([], |r| {
                Ok(BanlistEntry {
                    word: r.get(0)?,
                    added: r.get(1)?,
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

    /// Ban a word: add to banlist and remove any existing logged data for it.
    pub fn ban_word(&self, word: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp_millis();
        self.conn.execute(
            "INSERT INTO banlist(word, added) VALUES(?1, ?2)
             ON CONFLICT(word) DO NOTHING",
            params![word, now],
        )?;
        self.conn
            .execute("DELETE FROM words WHERE word = ?1", params![word])?;
        self.conn
            .execute("DELETE FROM chords WHERE phrase = ?1", params![word])?;
        Ok(())
    }

    pub fn unban_word(&self, word: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM banlist WHERE word = ?1", params![word])?;
        Ok(())
    }

    // --- Hidden words (display filter only — logged data is preserved) ------

    /// Hide a word from suggestions and the words list. Logged data is kept.
    pub fn hide_word(&self, word: &str) -> Result<()> {
        let w = word.trim().to_lowercase();
        let ts = chrono::Utc::now().timestamp_millis();
        self.conn.execute(
            "INSERT INTO hidden_words(word, added) VALUES(?1, ?2)
             ON CONFLICT(word) DO NOTHING",
            params![w, ts],
        )?;
        Ok(())
    }

    /// Remove a word from the hidden list (restores visibility).
    pub fn unhide_word(&self, word: &str) -> Result<()> {
        let w = word.trim().to_lowercase();
        self.conn
            .execute("DELETE FROM hidden_words WHERE word = ?1", params![w])?;
        Ok(())
    }

    /// List all hidden words (lowercase, sorted alphabetically).
    pub fn list_hidden(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Ok(mut stmt) = self
            .conn
            .prepare("SELECT word FROM hidden_words ORDER BY word ASC")
        {
            let _ = stmt.query_map([], |r| r.get::<_, String>(0)).map(|rows| {
                for row in rows.flatten() {
                    out.push(row);
                }
            });
        }
        out
    }

    // --- Device chords (written by the serial agent later) ----------------

    /// Return all chord phrases as a normalized (lowercased, trimmed) set.
    /// Used to build the in-memory lookup used by the detector thread.
    pub fn chord_phrase_set(&self) -> std::collections::HashSet<String> {
        let mut out = std::collections::HashSet::new();
        if let Ok(mut stmt) = self
            .conn
            .prepare("SELECT phrase FROM device_chords")
        {
            let _ = stmt.query_map([], |r| r.get::<_, String>(0)).map(|rows| {
                for row in rows.flatten() {
                    out.insert(row.trim().to_lowercase());
                }
            });
        }
        out
    }

    /// Replace all device chords for a given device id.
    pub fn replace_device_chords(
        &self,
        device_id: &str,
        chords: Vec<(String, Vec<u8>)>,
    ) -> Result<()> {
        self.conn.execute(
            "DELETE FROM device_chords WHERE device_id = ?1",
            params![device_id],
        )?;
        for (phrase, actions) in chords {
            self.conn.execute(
                "INSERT INTO device_chords(phrase, actions, device_id) VALUES(?1, ?2, ?3)",
                params![phrase, actions, device_id],
            )?;
        }
        Ok(())
    }
}
