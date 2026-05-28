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

    /// Practiceable single-word library phrases for the Sentence-mode vocab:
    /// distinct device-chord phrases that are a single token (no internal
    /// whitespace), all-alphabetic, and practiceable (no control chars / escape
    /// artifacts). Returned lowercased + deduped + sorted (stable order → stable
    /// grammar cache key).
    pub fn practiceable_words(&self) -> Vec<String> {
        let mut set = std::collections::BTreeSet::new();
        if let Ok(mut stmt) = self
            .conn
            .prepare("SELECT DISTINCT phrase FROM device_chords")
        {
            let _ = stmt.query_map([], |r| r.get::<_, String>(0)).map(|rows| {
                for phrase in rows.flatten() {
                    let t = phrase.trim();
                    // Single token only (no spaces), all alphabetic, non-empty.
                    if t.is_empty()
                        || t.contains(char::is_whitespace)
                        || !t.chars().all(|c| c.is_alphabetic())
                    {
                        continue;
                    }
                    set.insert(t.to_lowercase());
                }
            });
        }
        set.into_iter().collect()
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

    /// Load position → action_code for `device_id`, falling back to the most
    /// recent layout in the DB when this device has none stored (covers the case
    /// where the device isn't connected this session but a layout was persisted).
    /// Returns the map plus the device_id whose layout was actually used; an empty
    /// map (and empty id) means the DB has no layout at all.
    fn layout_pos_to_code(&self, device_id: &str) -> (std::collections::HashMap<u16, u16>, String) {
        let load = |id: &str| -> std::collections::HashMap<u16, u16> {
            let mut m = std::collections::HashMap::new();
            if let Ok(mut st) = self
                .conn
                .prepare("SELECT position, action_code FROM device_layout WHERE device_id = ?1")
            {
                if let Ok(rows) = st.query_map(params![id], |r| {
                    let pos: i64 = r.get(0)?;
                    let code: i64 = r.get(1)?;
                    Ok((pos as u16, code as u16))
                }) {
                    for (pos, code) in rows.flatten() {
                        m.insert(pos, code);
                    }
                }
            }
            m
        };

        let direct = load(device_id);
        if !direct.is_empty() {
            return (direct, device_id.to_string());
        }
        // Fall back to any stored layout so constraints still apply.
        let fallback_id: Option<String> = self
            .conn
            .query_row(
                "SELECT device_id FROM device_layout ORDER BY rowid DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .ok();
        match fallback_id {
            Some(fid) => {
                let m = load(&fid);
                (m, fid)
            }
            None => (std::collections::HashMap::new(), String::new()),
        }
    }

    /// Hardcoded joystick groups (position sets sharing one stick) for the device
    /// type inferred from `device_id`. Each inner slice lists a stick's positions
    /// in direction order. Left-hand sticks occupy the first half, right-hand the
    /// second half — so stick `k` mirrors stick `k + len/2` at the same direction
    /// index. Derived from the DeviceManager layout YML files.
    fn joystick_groups(device_id: &str) -> &'static [&'static [u16]] {
        if device_id.contains("M4G") || device_id.contains("CCX") || device_id.contains("CCB") {
            // M4G / CCX / M4GR: 4-direction joysticks, 16 groups (8 per hand).
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
            // CC1 / Lite / default: 5-direction joysticks, 18 groups (9 per hand).
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
        }
    }

    /// Returns a map from action_code → joystick_group_id using the stored layout
    /// and hardcoded joystick groups for the device type inferred from device_id.
    ///
    /// If no layout is stored for `device_id`, falls back to any layout in the DB
    /// (covers the common case where the device is not connected in this session but
    /// layout was persisted from a prior connection). Returns an empty map only when
    /// the DB has no layout at all.
    pub fn action_to_joystick_group(&self, device_id: &str) -> std::collections::HashMap<u16, usize> {
        let (pos_to_code, effective_device_id) = self.layout_pos_to_code(device_id);
        if pos_to_code.is_empty() {
            return std::collections::HashMap::new();
        }

        let groups = Self::joystick_groups(&effective_device_id);

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

    /// Returns a map from action_code → mirror action_code: the key at the same
    /// direction on the mirror-hand stick. A stick in the first half of the groups
    /// (left hand) pairs with the stick at the same offset in the second half
    /// (right hand); within a stick, direction index maps straight across.
    ///
    /// Used as a fallback when the wanted key can't be pressed (its thumb is busy
    /// or its combo is taken): e.g. `p` (right) → `v` (left). Bidirectional.
    /// Empty when the DB has no layout.
    pub fn action_mirror_map(&self, device_id: &str) -> std::collections::HashMap<u16, u16> {
        let (pos_to_code, effective_device_id) = self.layout_pos_to_code(device_id);
        if pos_to_code.is_empty() {
            return std::collections::HashMap::new();
        }

        let groups = Self::joystick_groups(&effective_device_id);
        let half = groups.len() / 2;
        let mut result = std::collections::HashMap::new();
        for k in 0..half {
            let left = groups[k];
            let right = groups[k + half];
            for i in 0..left.len().min(right.len()) {
                if let (Some(&lc), Some(&rc)) =
                    (pos_to_code.get(&left[i]), pos_to_code.get(&right[i]))
                {
                    result.insert(lc, rc);
                    result.insert(rc, lc);
                }
            }
        }
        result
    }

    /// Group indices of the (index, middle, ring) finger sticks for the left and
    /// right hands, or `None` for device types whose finger layout isn't
    /// characterized (the ergonomic check stays inert rather than guessing).
    fn finger_stick_groups(device_id: &str) -> Option<([usize; 3], [usize; 3])> {
        if device_id.contains("M4G") || device_id.contains("CCX") || device_id.contains("CCB") {
            // Per hand the sticks run thumb(0,1) → index, middle, ring. Confirmed
            // on M4G: right index=10 (t), middle=11 (l/j), ring=12 (s).
            Some(([2, 3, 4], [10, 11, 12]))
        } else {
            None
        }
    }

    /// Returns a map from action_code → (hand, finger, horizontal) for the three
    /// letter fingers per hand. `hand`: 0=left, 1=right. `finger`: 0=index,
    /// 1=middle, 2=ring. `horizontal`: true when the key's direction is left/right
    /// (stick index 0 or 2) rather than up/down — the directions that make the
    /// middle finger crowd a neighbor. Empty when layout or finger map is unknown.
    pub fn action_finger_map(
        &self,
        device_id: &str,
    ) -> std::collections::HashMap<u16, (u8, u8, bool)> {
        let mut out = std::collections::HashMap::new();
        let (pos_to_code, effective_device_id) = self.layout_pos_to_code(device_id);
        if pos_to_code.is_empty() {
            return out;
        }
        let Some((left_trio, right_trio)) = Self::finger_stick_groups(&effective_device_id) else {
            return out;
        };
        let groups = Self::joystick_groups(&effective_device_id);
        for (hand, trio) in [(0u8, left_trio), (1u8, right_trio)] {
            for (finger, &g) in trio.iter().enumerate() {
                let Some(positions) = groups.get(g) else { continue };
                for (idx, &pos) in positions.iter().enumerate() {
                    if let Some(&code) = pos_to_code.get(&pos) {
                        let horizontal = idx == 0 || idx == 2;
                        out.insert(code, (hand, finger as u8, horizontal));
                    }
                }
            }
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
