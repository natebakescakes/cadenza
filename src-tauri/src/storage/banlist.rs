use anyhow::Result;
use rusqlite::params;

use crate::types::BanlistEntry;

use super::Storage;

impl Storage {
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
}
