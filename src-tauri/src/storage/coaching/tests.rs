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
    // Two device_chords rows for the same phrase → alt_count = rows - 1 = 1.
    add_device_chord(&s, "the", &[b'b', b'a'], "dev-1"); // decodes "a + b"
    add_device_chord(&s, "the", &[b'c', b'd'], "dev-1"); // decodes "c + d"

    let m = s
        .coaching_mapping("the", Some("dev-1"))
        .expect("device mapping present");
    assert_eq!(m.source, "device");
    assert_eq!(m.primary, "a + b");
    assert_eq!(m.alt_count, 1);
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
    assert_eq!(m.alt_count, 0);
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
fn v_unit2b_suggested_below_min_len_suppressed() {
    let s = Storage::open_in_memory();
    // A frequent 2-char token (e.g. a mouseless grid label typed repeatedly):
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
    // Default min_len = 3: "fj" (len 2) suppressed, "the" (len 3) shown.
    assert!(!s.coaching_should_show("fj", "suggested", &settings()));
    assert!(s.coaching_should_show("the", "suggested", &settings()));
    // Lowering min_len to 2 re-enables the 2-char suggestion.
    let s2 = Settings {
        coaching_suggest_min_len: 2,
        ..Settings::default()
    };
    assert!(s.coaching_should_show("fj", "suggested", &s2));
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
