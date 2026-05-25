// SQLite persistence layer (rusqlite, plain `bundled`).
//
// DB path (macOS): `~/Library/Application Support/Cadenza/cadenza.sqlite3`.
//
// Encryption path chosen: plain bundled SQLite + Argon2 password hash stored in
// a `meta` table. This keeps the build green (no OpenSSL/SQLCipher) while still
// implementing the dbInit/dbUnlock UX (set/verify password). Real at-rest
// encryption (SQLCipher PRAGMA key) can be layered in later without changing
// the public API of this module.

mod banlist;
mod coaching;
mod combos;
mod device;
mod queries;
mod writes;

// Re-exported for the command layer / later overlay phases. The mapping type is
// the return value of the public `coaching_mapping` method.
#[allow(unused_imports)]
pub use coaching::CoachingMapping;

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rusqlite::{params, Connection, OptionalExtension};

/// Estimated time a chord takes to fire on-device (ms). Used for suggestion savings.
pub(super) const ESTIMATED_CHORD_MS: f64 = 150.0;

/// Gaps longer than this between consecutive units are treated as idle/thinking
/// time and capped, so they don't deflate the active-typing denominator.
pub(super) const IDLE_CAP_MS: i64 = 5000;

/// A gap larger than this between units starts a new "session" for the session WPM number.
pub(super) const SESSION_GAP_MS: i64 = 30_000;

/// A single logged unit pulled from `wpm_samples` for query-time WPM computation.
pub(super) struct RawUnit {
    pub(super) t: i64,
    pub(super) chars: i64,
    pub(super) source: String,
}

/// Active-typing milliseconds across an ordered slice of units: sum of capped
/// gaps to the previous unit, with a floor to avoid divide-by-zero / spikes.
pub(super) fn active_ms(units: &[RawUnit]) -> f64 {
    let mut total: i64 = 0;
    for w in units.windows(2) {
        let gap = (w[1].t - w[0].t).max(0);
        total += gap.min(IDLE_CAP_MS);
    }
    (total as f64).max(1000.0)
}

/// chars/5 over active minutes for the given units.
pub(super) fn wpm_of(units: &[RawUnit]) -> f64 {
    if units.is_empty() {
        return 0.0;
    }
    let chars: i64 = units.iter().map(|u| u.chars).sum();
    let words = chars as f64 / 5.0;
    words / (active_ms(units) / 60000.0)
}

/// Owns the open SQLite connection.
pub struct Storage {
    pub(super) conn: Connection,
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
    pub(super) fn raw_units(&self, since: i64) -> Vec<RawUnit> {
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
            CREATE TABLE IF NOT EXISTS device_layout (
                device_id TEXT NOT NULL,
                position  INTEGER NOT NULL,
                action_code INTEGER NOT NULL,
                PRIMARY KEY (device_id, position)
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
        // Migration: chord sub-kind ("chord" | "arpeggio") for activity-feed distinction.
        // Idempotent: ignore duplicate-column error.
        let _ = conn.execute(
            "ALTER TABLE chords ADD COLUMN kind TEXT DEFAULT 'chord'",
            [],
        );
        // Migration: separate deletion_count from error_count in chord_errors.
        // deletion_count = BS-count signal (lower confidence); error_count = retype signal.
        let _ = conn.execute(
            "ALTER TABLE chord_errors ADD COLUMN deletion_count INTEGER DEFAULT 0",
            [],
        );
        // Migration: split_phrases — consecutive manual flushes < 3s whose concat
        // is a known word/chord phrase (e.g. "under"+"lying" → "underlying").
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS split_phrases (
                phrase      TEXT PRIMARY KEY,
                split_count INTEGER NOT NULL DEFAULT 0,
                last_split  INTEGER NOT NULL DEFAULT 0
            );",
        );
        // Migration: confusion_count — chord deleted then a different chord fired within window.
        let _ = conn.execute(
            "ALTER TABLE chord_errors ADD COLUMN confusion_count INTEGER DEFAULT 0",
            [],
        );
        // Migration: mastered_at — epoch-ms timestamp when a chord's phrase first
        // passed the mastery gate (stamped on the chord-fire path). Persisted so a
        // later regression (usage_rate drop) can re-surface the coaching reminder;
        // "previously-mastered" is NOT derivable from live metrics alone.
        // Idempotent: ignore the duplicate-column error if it already exists.
        let _ = conn.execute(
            "ALTER TABLE chord_manual ADD COLUMN mastered_at INTEGER DEFAULT NULL",
            [],
        );
        Ok(())
    }

    /// Test-only: open an in-memory DB with the full schema/migrations applied.
    #[cfg(test)]
    pub(crate) fn open_in_memory() -> Self {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        Self::create_schema(&conn).expect("create schema");
        Self { conn }
    }

    /// Test-only: run `create_schema` (incl. idempotent migrations) against an
    /// arbitrary connection. Exposed so the migration test can build a
    /// pre-migration schema, then apply migrations and re-apply for idempotency.
    #[cfg(test)]
    pub(crate) fn create_schema_for_test(conn: &Connection) -> Result<()> {
        Self::create_schema(conn)
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

    // --- is_banned (used by detector thread) ------------------------------

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
}

