// The recommend-only "chords to add" queue. A locally-curated list of
// (phrase, combo) pairs the user might want to add to their device. This layer
// NEVER writes to the CharaChorder — it only stores the user's manual picks.

use rusqlite::params;

use crate::types::ChordRecommendation;

use super::super::Storage;

impl Storage {
    /// Add (or refresh) a recommendation. UPSERT on (phrase, combo): re-adding an
    /// existing pair just bumps its `created_at` so it floats to the top of the
    /// created_at-DESC list. `created_at` is unix epoch seconds.
    pub fn add_chord_recommendation(&self, phrase: &str, combo: &str) -> rusqlite::Result<()> {
        let created_at = chrono::Utc::now().timestamp();
        self.conn.execute(
            "INSERT INTO chord_recommendations(phrase, combo, created_at) VALUES(?1, ?2, ?3)
             ON CONFLICT(phrase, combo) DO UPDATE SET created_at = excluded.created_at",
            params![phrase, combo, created_at],
        )?;
        Ok(())
    }

    /// All recommendations, newest first.
    pub fn list_chord_recommendations(&self) -> rusqlite::Result<Vec<ChordRecommendation>> {
        let mut stmt = self.conn.prepare(
            "SELECT phrase, combo, created_at FROM chord_recommendations ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ChordRecommendation {
                phrase: r.get(0)?,
                combo: r.get(1)?,
                created_at: r.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Remove a single recommendation by its (phrase, combo) key.
    pub fn remove_chord_recommendation(&self, phrase: &str, combo: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM chord_recommendations WHERE phrase = ?1 AND combo = ?2",
            params![phrase, combo],
        )?;
        Ok(())
    }

    /// Clear the entire recommendation queue.
    pub fn clear_chord_recommendations(&self) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM chord_recommendations", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::Storage;

    #[test]
    fn add_list_remove_and_upsert() {
        let s = Storage::open_in_memory();

        // add → list
        s.add_chord_recommendation("hello", "h + e + l").unwrap();
        s.add_chord_recommendation("world", "w + o + r").unwrap();
        let rows = s.list_chord_recommendations().unwrap();
        assert_eq!(rows.len(), 2);

        // UPSERT: re-adding the same (phrase, combo) keeps a single row.
        s.add_chord_recommendation("hello", "h + e + l").unwrap();
        let rows = s.list_chord_recommendations().unwrap();
        assert_eq!(rows.len(), 2);

        // remove → list
        s.remove_chord_recommendation("hello", "h + e + l").unwrap();
        let rows = s.list_chord_recommendations().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].phrase, "world");
        assert_eq!(rows[0].combo, "w + o + r");

        // clear → empty
        s.clear_chord_recommendations().unwrap();
        assert!(s.list_chord_recommendations().unwrap().is_empty());
    }
}
