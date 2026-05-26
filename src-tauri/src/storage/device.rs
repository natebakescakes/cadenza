use anyhow::Result;
use rusqlite::params;

use super::Storage;

impl Storage {
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

    /// Replace all layout entries for a device.
    pub fn replace_device_layout(
        &self,
        device_id: &str,
        entries: Vec<(u16, u16)>,
    ) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "DELETE FROM device_layout WHERE device_id = ?1",
            params![device_id],
        )?;
        for (pos, code) in entries {
            self.conn.execute(
                "INSERT INTO device_layout(device_id, position, action_code) VALUES(?1, ?2, ?3)",
                params![device_id, pos as i64, code as i64],
            )?;
        }
        Ok(())
    }

    /// Returns a map from action_code → joystick_group_id using the stored layout
    /// and hardcoded joystick groups for the device type inferred from device_id.
    ///
    /// If no layout is stored for `device_id`, falls back to any layout in the DB
    /// (covers the common case where the device is not connected in this session but
    /// layout was persisted from a prior connection). Returns an empty map only when
    /// the DB has no layout at all.
    pub fn action_to_joystick_group(&self, device_id: &str) -> std::collections::HashMap<u16, usize> {
        // Load position → action_code from DB.
        let mut pos_to_code: std::collections::HashMap<u16, u16> =
            std::collections::HashMap::new();
        if let Ok(mut st) = self
            .conn
            .prepare("SELECT position, action_code FROM device_layout WHERE device_id = ?1")
        {
            if let Ok(rows) = st.query_map(params![device_id], |r| {
                let pos: i64 = r.get(0)?;
                let code: i64 = r.get(1)?;
                Ok((pos as u16, code as u16))
            }) {
                for (pos, code) in rows.flatten() {
                    pos_to_code.insert(pos, code);
                }
            }
        }

        // If no layout for this device_id, fall back to any stored layout so
        // joystick constraints still apply (e.g. device not connected this session).
        let effective_device_id: String = if pos_to_code.is_empty() {
            let fallback_id: Option<String> = self
                .conn
                .query_row(
                    "SELECT device_id FROM device_layout ORDER BY rowid DESC LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .ok();
            match fallback_id {
                None => return std::collections::HashMap::new(),
                Some(fid) => {
                    if let Ok(mut st) = self.conn.prepare(
                        "SELECT position, action_code FROM device_layout WHERE device_id = ?1",
                    ) {
                        if let Ok(rows) = st.query_map(params![&fid], |r| {
                            let pos: i64 = r.get(0)?;
                            let code: i64 = r.get(1)?;
                            Ok((pos as u16, code as u16))
                        }) {
                            for (pos, code) in rows.flatten() {
                                pos_to_code.insert(pos, code);
                            }
                        }
                    }
                    fid
                }
            }
        } else {
            device_id.to_string()
        };

        if pos_to_code.is_empty() {
            return std::collections::HashMap::new();
        }

        // Hardcoded joystick groups (position sets that share a joystick).
        // Derived from the layout YML files in DeviceManager.
        let groups: &[&[u16]] = if effective_device_id.contains("M4G") || effective_device_id.contains("CCX") || effective_device_id.contains("CCB") {
            // M4G / CCX / M4GR: 4-direction joysticks, 16 groups
            &[
                &[6, 7, 8, 9],
                &[11, 12, 13, 14],
                &[16, 17, 18, 19],
                &[21, 22, 23, 24],
                &[26, 27, 28, 29],
                &[31, 32, 33, 34],
                &[36, 37, 38, 39],
                &[41, 42, 43, 44],
                &[51, 52, 53, 54],
                &[56, 57, 58, 59],
                &[61, 62, 63, 64],
                &[66, 67, 68, 69],
                &[71, 72, 73, 74],
                &[76, 77, 78, 79],
                &[81, 82, 83, 84],
                &[86, 87, 88, 89],
            ]
        } else {
            // CC1 / Lite / default: 5-direction joysticks, 18 groups
            &[
                &[0, 1, 2, 3, 4],
                &[5, 6, 7, 8, 9],
                &[10, 11, 12, 13, 14],
                &[15, 16, 17, 18, 19],
                &[20, 21, 22, 23, 24],
                &[25, 26, 27, 28, 29],
                &[30, 31, 32, 33, 34],
                &[35, 36, 37, 38, 39],
                &[40, 41, 42, 43, 44],
                &[45, 46, 47, 48, 49],
                &[50, 51, 52, 53, 54],
                &[55, 56, 57, 58, 59],
                &[60, 61, 62, 63, 64],
                &[65, 66, 67, 68, 69],
                &[70, 71, 72, 73, 74],
                &[75, 76, 77, 78, 79],
                &[80, 81, 82, 83, 84],
                &[85, 86, 87, 88, 89],
            ]
        };

        // Build action_code → group_id map.
        let mut result = std::collections::HashMap::new();
        for (group_id, positions) in groups.iter().enumerate() {
            for &pos in *positions {
                if let Some(&code) = pos_to_code.get(&pos) {
                    result.insert(code, group_id);
                }
            }
        }
        result
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
