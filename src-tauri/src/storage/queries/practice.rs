// Spaced-repetition practice queries (the "practice hub").
//
// FULL ISOLATION GUARANTEE: every query in this file touches ONLY the
// practice_* tables (practice_cards / practice_sessions / practice_attempts).
// It NEVER reads or writes the ambient stats tables (chords, chord_manual,
// chord_errors, words, wpm_samples). The single cross-read is `self.proficiency()`
// — a read-only ranking used to SEED which weak chords enter the practice queue;
// it mutates nothing.

use rusqlite::{params, params_from_iter, OptionalExtension};

use crate::types::{PracticeAttemptSummary, PracticeCard, PracticeCardStats, PracticeOverview};

use super::super::Storage;

/// Lower SM-2 ease floor. Standard SM-2 never lets ease drop below 1.3.
const EASE_FLOOR: f64 = 1.3;

/// fire_ms below this on a first-try-correct attempt earns the top grade (5).
/// Chosen as a "snappy chord" threshold: genuine chord bursts fire well under
/// this, while a hesitant-but-correct first try lands above it (grade 4). Tune
/// against observed practice_attempts.fire_ms if drills feel mis-graded.
const FAST_FIRE_MS: f64 = 600.0;

/// Standard SM-2 review step.
///
/// `grade` is the 0-5 quality of recall. grade < 3 is a LAPSE: reps reset to 0,
/// lapses increment, and the interval drops to a short relearning step (1 day).
/// grade >= 3 ADVANCES: reps increment and the interval grows
/// (rep 1 -> 1d, rep 2 -> 6d, thereafter -> prev_interval * ease). Ease is
/// adjusted by the standard formula and floored at 1.3.
///
/// Returns `(new_ease, new_interval_days, new_reps, new_lapses)`.
pub fn sm2_review(
    ease: f64,
    interval_days: f64,
    reps: i64,
    lapses: i64,
    grade: u8,
) -> (f64, f64, i64, i64) {
    let g = grade as f64;
    // Standard SM-2 ease-factor adjustment, floored at EASE_FLOOR.
    let new_ease = (ease + (0.1 - (5.0 - g) * (0.08 + (5.0 - g) * 0.02))).max(EASE_FLOOR);

    if grade < 3 {
        // Lapse: reset reps, bump lapses, short relearning interval.
        (new_ease, 1.0, 0, lapses + 1)
    } else {
        let new_reps = reps + 1;
        let new_interval = match new_reps {
            1 => 1.0,
            2 => 6.0,
            _ => (interval_days * new_ease).max(1.0),
        };
        (new_ease, new_interval, new_reps, lapses)
    }
}

/// Derive an SM-2 grade (0-5) from a single practice attempt result.
///
/// - wrong / not fired      -> 2 (lapse: keeps cards in rotation without zeroing ease too hard).
/// - correct, not first try -> 3 (passing, minimal advance).
/// - correct, first try     -> 4 (good).
/// - correct, first try, fast (< FAST_FIRE_MS) -> 5 (easy).
pub fn grade_from_result(correct: bool, first_try: bool, fire_ms: f64) -> u8 {
    if !correct {
        return 2;
    }
    if !first_try {
        return 3;
    }
    if fire_ms > 0.0 && fire_ms < FAST_FIRE_MS {
        5
    } else {
        4
    }
}

/// Number of ms per day (for due_at scheduling).
const MS_PER_DAY: f64 = 86_400_000.0;

/// How many weak-chord seed candidates we examine from `proficiency()` per queue
/// fill. Bounds the cross-read so a large chordmap can't blow up the queue cost.
const SEED_SCAN_LIMIT: usize = 200;

impl Storage {
    /// Decode the device combo strings for a phrase (read-only; device_chords is
    /// not an ambient stats table — it's the static chord map).
    fn practice_combos(&self, phrase: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Ok(mut stmt) = self
            .conn
            .prepare("SELECT actions FROM device_chords WHERE LOWER(phrase) = LOWER(?1)")
        {
            if let Ok(rows) = stmt.query_map(params![phrase], |r| r.get::<_, Vec<u8>>(0)) {
                for blob in rows.flatten() {
                    let combo = super::super::combos::decode_actions_blob(&blob);
                    if !combo.is_empty() {
                        out.push(combo);
                    }
                }
            }
        }
        out
    }

    /// Count cards due now: existing cards with due_at <= now, PLUS brand-new
    /// weak-chord seed candidates (phrases with no card yet) — those are treated
    /// as immediately due so a fresh user has something to drill.
    pub fn practice_due_count(&self, now_ms: i64) -> i64 {
        let existing_due: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM practice_cards WHERE due_at <= ?1",
                params![now_ms],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
            .unwrap_or(0);

        // New seed candidates: weak phrases from proficiency() that have no card
        // yet. Resolve "which already have a card" in ONE query (not a per-phrase
        // N+1 — that loop, polled from the dashboard, piled up under the storage
        // mutex and pegged the CPU).
        let candidates: Vec<String> = self
            .proficiency()
            .into_iter()
            .take(SEED_SCAN_LIMIT)
            .map(|p| p.phrase)
            .collect();
        let new_count = candidates.len() as i64 - self.count_carded(&candidates);
        existing_due + new_count
    }

    /// Which of `phrases` already have a practice_cards row — one query.
    fn carded_set(&self, phrases: &[String]) -> std::collections::HashSet<String> {
        let mut set = std::collections::HashSet::new();
        if phrases.is_empty() {
            return set;
        }
        let placeholders = vec!["?"; phrases.len()].join(",");
        let sql = format!("SELECT phrase FROM practice_cards WHERE phrase IN ({placeholders})");
        if let Ok(mut stmt) = self.conn.prepare(&sql) {
            if let Ok(rows) = stmt.query_map(params_from_iter(phrases.iter()), |r| {
                r.get::<_, String>(0)
            }) {
                for p in rows.flatten() {
                    set.insert(p);
                }
            }
        }
        set
    }

    /// How many of `phrases` already have a practice_cards row.
    fn count_carded(&self, phrases: &[String]) -> i64 {
        self.carded_set(phrases).len() as i64
    }


    /// Build the due queue: due existing cards first (due_at asc), then the top
    /// weak phrases from `proficiency()` that have NO card yet (seed candidates),
    /// up to `limit`.
    pub fn practice_due_queue(&self, now_ms: i64, limit: i64) -> Vec<PracticeCard> {
        let limit = limit.max(0) as usize;
        let mut out: Vec<PracticeCard> = Vec::new();
        if limit == 0 {
            return out;
        }

        // 1. Due existing cards, soonest-due first.
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT phrase, ease, interval_days, due_at, reps, lapses, last_reviewed
             FROM practice_cards
             WHERE due_at <= ?1
             ORDER BY due_at ASC
             LIMIT ?2",
        ) {
            if let Ok(rows) = stmt.query_map(params![now_ms, limit as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                ))
            }) {
                for (phrase, ease, interval_days, due_at, reps, lapses, last_reviewed) in
                    rows.flatten()
                {
                    let combos = self.practice_combos(&phrase);
                    out.push(PracticeCard {
                        phrase,
                        combos,
                        ease,
                        interval_days,
                        due_at,
                        reps,
                        lapses,
                        last_reviewed,
                        is_new: false,
                    });
                }
            }
        }

        // 2. Seed candidates: weakest phrases with no card yet, until we hit limit.
        if out.len() < limit {
            let candidates: Vec<_> =
                self.proficiency().into_iter().take(SEED_SCAN_LIMIT).collect();
            // Resolve which candidates already have a card in ONE query (not a
            // per-candidate N+1 — that ran on the main thread and spiked CPU).
            let phrases: Vec<String> = candidates.iter().map(|p| p.phrase.clone()).collect();
            let carded = self.carded_set(&phrases);
            for prof in candidates {
                if out.len() >= limit {
                    break;
                }
                if carded.contains(&prof.phrase) {
                    continue;
                }
                // Skip if already queued as a (theoretically impossible) duplicate.
                if out.iter().any(|c| c.phrase == prof.phrase) {
                    continue;
                }
                let combos = if prof.combos.is_empty() {
                    self.practice_combos(&prof.phrase)
                } else {
                    prof.combos.clone()
                };
                out.push(PracticeCard {
                    phrase: prof.phrase,
                    combos,
                    ease: 2.5,
                    interval_days: 0.0,
                    due_at: now_ms,
                    reps: 0,
                    lapses: 0,
                    last_reviewed: 0,
                    is_new: true,
                });
            }
        }

        out
    }

    /// Open a new practice session row, returning its id.
    pub fn practice_start_session(&self, now_ms: i64) -> i64 {
        match self.conn.execute(
            "INSERT INTO practice_sessions (started_at, completed_at) VALUES (?1, NULL)",
            params![now_ms],
        ) {
            Ok(_) => self.conn.last_insert_rowid(),
            Err(_) => 0,
        }
    }

    /// Mark a practice session complete.
    pub fn practice_complete_session(&self, session_id: i64, now_ms: i64) {
        let _ = self.conn.execute(
            "UPDATE practice_sessions SET completed_at = ?2 WHERE id = ?1",
            params![session_id, now_ms],
        );
    }

    /// Log a single practice attempt (raw row; does not update the SM-2 card).
    pub fn practice_log_attempt(
        &self,
        session_id: i64,
        phrase: &str,
        correct: bool,
        first_try: bool,
        fire_ms: f64,
        backspaces: i64,
        corrections: i64,
        hint_used: bool,
        now_ms: i64,
    ) {
        let _ = self.conn.execute(
            "INSERT INTO practice_attempts
                (session_id, phrase, correct, first_try, fire_ms, backspaces, corrections, hint_used, ts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                session_id,
                phrase,
                correct as i64,
                first_try as i64,
                fire_ms,
                backspaces,
                corrections,
                hint_used as i64,
                now_ms
            ],
        );
    }

    /// Apply a practice result to the SM-2 card: derive grade, advance/lapse via
    /// `sm2_review`, and upsert the row with the new due_at. Creates the card with
    /// SM-2 defaults if it does not exist yet.
    pub fn practice_submit_result(
        &self,
        phrase: &str,
        correct: bool,
        first_try: bool,
        fire_ms: f64,
        now_ms: i64,
    ) {
        // Read current card state (defaults for a brand-new card).
        let (ease, interval_days, reps, lapses) = self
            .conn
            .query_row(
                "SELECT ease, interval_days, reps, lapses FROM practice_cards WHERE phrase = ?1",
                params![phrase],
                |r| {
                    Ok((
                        r.get::<_, f64>(0)?,
                        r.get::<_, f64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .ok()
            .flatten()
            .unwrap_or((2.5, 0.0, 0, 0));

        let grade = grade_from_result(correct, first_try, fire_ms);
        let (new_ease, new_interval, new_reps, new_lapses) =
            sm2_review(ease, interval_days, reps, lapses, grade);
        let due_at = now_ms + (new_interval * MS_PER_DAY) as i64;

        let _ = self.conn.execute(
            "INSERT INTO practice_cards
                (phrase, ease, interval_days, due_at, reps, lapses, last_reviewed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(phrase) DO UPDATE SET
                ease          = excluded.ease,
                interval_days = excluded.interval_days,
                due_at        = excluded.due_at,
                reps          = excluded.reps,
                lapses        = excluded.lapses,
                last_reviewed = excluded.last_reviewed",
            params![
                phrase,
                new_ease,
                new_interval,
                due_at,
                new_reps,
                new_lapses,
                now_ms
            ],
        );
    }

    /// Per-card practice statistics (card state + recent avg fire_ms + first-try
    /// accuracy from practice_attempts).
    pub fn practice_card_stats(&self, phrase: &str) -> PracticeCardStats {
        let mut stats = PracticeCardStats {
            phrase: phrase.to_string(),
            ease: 2.5,
            ..PracticeCardStats::default()
        };

        if let Some((ease, interval_days, due_at, reps, lapses)) = self
            .conn
            .query_row(
                "SELECT ease, interval_days, due_at, reps, lapses
                 FROM practice_cards WHERE phrase = ?1",
                params![phrase],
                |r| {
                    Ok((
                        r.get::<_, f64>(0)?,
                        r.get::<_, f64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()
            .ok()
            .flatten()
        {
            stats.ease = ease;
            stats.interval_days = interval_days;
            stats.due_at = due_at;
            stats.reps = reps;
            stats.lapses = lapses;
        }

        // Recent avg fire_ms over the last 20 attempts (correct fires carry timing).
        stats.recent_avg_fire_ms = self
            .conn
            .query_row(
                "SELECT AVG(fire_ms) FROM (
                     SELECT fire_ms FROM practice_attempts
                     WHERE phrase = ?1 AND correct = 1
                     ORDER BY ts DESC LIMIT 20
                 )",
                params![phrase],
                |r| r.get::<_, Option<f64>>(0),
            )
            .optional()
            .ok()
            .flatten()
            .flatten()
            .unwrap_or(0.0);

        // First-try accuracy: first_try-correct attempts / total attempts.
        let (total, first_try_correct): (i64, i64) = self
            .conn
            .query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(CASE WHEN correct = 1 AND first_try = 1 THEN 1 ELSE 0 END), 0)
                 FROM practice_attempts WHERE phrase = ?1",
                params![phrase],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .ok()
            .flatten()
            .unwrap_or((0, 0));
        stats.first_try_accuracy = if total > 0 {
            first_try_correct as f64 / total as f64
        } else {
            0.0
        };

        // Mean backspaces over the most recent 20 attempts (mirrors recent_avg_fire_ms's window).
        stats.avg_backspaces = self
            .conn
            .query_row(
                "SELECT AVG(backspaces) FROM (
                     SELECT backspaces FROM practice_attempts
                     WHERE phrase = ?1
                     ORDER BY ts DESC LIMIT 20
                 )",
                params![phrase],
                |r| r.get::<_, Option<f64>>(0),
            )
            .optional()
            .ok()
            .flatten()
            .flatten()
            .unwrap_or(0.0);

        // Clean rate: fraction of all attempts with no backspaces AND no corrections.
        let clean: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(CASE WHEN backspaces = 0 AND corrections = 0 THEN 1 ELSE 0 END), 0)
                 FROM practice_attempts WHERE phrase = ?1",
                params![phrase],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
            .unwrap_or(0);
        stats.clean_rate = if total > 0 {
            clean as f64 / total as f64
        } else {
            0.0
        };

        // Best (minimum) fire_ms among correct attempts (0.0 if none).
        stats.best_fire_ms = self
            .conn
            .query_row(
                "SELECT MIN(fire_ms) FROM practice_attempts
                 WHERE phrase = ?1 AND correct = 1",
                params![phrase],
                |r| r.get::<_, Option<f64>>(0),
            )
            .optional()
            .ok()
            .flatten()
            .flatten()
            .unwrap_or(0.0);

        // Hint rate: fraction of all attempts where a hint was used.
        let hinted: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(CASE WHEN hint_used = 1 THEN 1 ELSE 0 END), 0)
                 FROM practice_attempts WHERE phrase = ?1",
                params![phrase],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
            .unwrap_or(0);
        stats.hint_rate = if total > 0 {
            hinted as f64 / total as f64
        } else {
            0.0
        };

        stats
    }

    /// Aggregate practice overview: total attempts, distinct cards, current
    /// streak (consecutive days ending today with >=1 completed session), and
    /// due count.
    pub fn practice_overview(&self, now_ms: i64) -> PracticeOverview {
        let total_reps: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM practice_attempts", [], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
            .unwrap_or(0);

        let distinct_cards: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM practice_cards", [], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
            .unwrap_or(0);

        let current_streak = self.practice_streak(now_ms);
        let due_count = self.practice_due_count(now_ms);

        PracticeOverview {
            total_reps,
            distinct_cards,
            current_streak,
            due_count,
        }
    }

    /// All attempts logged under one session, oldest-first — a post-session
    /// recap. Decodes the 0/1 integer flags (correct/first_try/hint_used) to bool.
    pub fn practice_session_summary(&self, session_id: i64) -> Vec<PracticeAttemptSummary> {
        let mut out = Vec::new();
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT phrase, correct, first_try, fire_ms, backspaces, corrections, hint_used, ts
             FROM practice_attempts
             WHERE session_id = ?1
             ORDER BY ts ASC",
        ) {
            if let Ok(rows) = stmt.query_map(params![session_id], |r| {
                Ok(PracticeAttemptSummary {
                    phrase: r.get::<_, String>(0)?,
                    correct: r.get::<_, i64>(1)? != 0,
                    first_try: r.get::<_, i64>(2)? != 0,
                    fire_ms: r.get::<_, f64>(3)?,
                    backspaces: r.get::<_, i64>(4)?,
                    corrections: r.get::<_, i64>(5)?,
                    hint_used: r.get::<_, i64>(6)? != 0,
                    ts: r.get::<_, i64>(7)?,
                })
            }) {
                for row in rows.flatten() {
                    out.push(row);
                }
            }
        }
        out
    }

    /// Consecutive days (ending today, in UTC day buckets) with >=1 completed
    /// session. Walks completed_at timestamps newest-first; the streak holds as
    /// long as each completed day is the current or previous expected day.
    fn practice_streak(&self, now_ms: i64) -> i64 {
        // Distinct UTC days (epoch-day integers) that had a completed session,
        // newest first.
        let mut days: Vec<i64> = Vec::new();
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT DISTINCT completed_at / 86400000
             FROM practice_sessions
             WHERE completed_at IS NOT NULL
             ORDER BY 1 DESC",
        ) {
            if let Ok(rows) = stmt.query_map([], |r| r.get::<_, i64>(0)) {
                for d in rows.flatten() {
                    days.push(d);
                }
            }
        }
        if days.is_empty() {
            return 0;
        }

        let today = now_ms / 86_400_000;
        // The streak only counts if the most recent completed day is today or
        // yesterday (otherwise it has lapsed).
        let mut expected = today;
        if days[0] != today {
            if days[0] == today - 1 {
                expected = today - 1;
            } else {
                return 0;
            }
        }

        let mut streak = 0;
        for d in days {
            if d == expected {
                streak += 1;
                expected -= 1;
            } else if d < expected {
                break;
            }
            // d > expected (duplicate already collapsed by DISTINCT) — skip.
        }
        streak
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sm2_advances_on_pass() {
        // Fresh card, grade 4: rep 1 -> interval 1 day, reps 1.
        let (ease, interval, reps, lapses) = sm2_review(2.5, 0.0, 0, 0, 4);
        assert_eq!(reps, 1);
        assert_eq!(lapses, 0);
        assert_eq!(interval, 1.0);
        assert!((ease - 2.5).abs() < 1e-9, "grade 4 keeps ease ~unchanged");

        // Second pass -> 6 days.
        let (_, interval2, reps2, _) = sm2_review(ease, interval, reps, lapses, 4);
        assert_eq!(reps2, 2);
        assert_eq!(interval2, 6.0);
    }

    #[test]
    fn sm2_lapses_on_fail() {
        let (ease, interval, reps, lapses) = sm2_review(2.5, 6.0, 2, 0, 1);
        assert_eq!(reps, 0, "lapse resets reps");
        assert_eq!(lapses, 1);
        assert_eq!(interval, 1.0, "lapse drops to short relearning interval");
        assert!(ease >= EASE_FLOOR);
    }

    #[test]
    fn sm2_ease_floored() {
        // Repeated low grades must never drive ease below 1.3.
        let mut ease = 2.5;
        for _ in 0..20 {
            let (e, _, _, _) = sm2_review(ease, 1.0, 0, 0, 0);
            ease = e;
        }
        assert!(ease >= EASE_FLOOR);
    }

    #[test]
    fn grade_thresholds() {
        assert_eq!(grade_from_result(false, false, 100.0), 2);
        assert_eq!(grade_from_result(false, true, 100.0), 2);
        assert_eq!(grade_from_result(true, false, 100.0), 3);
        assert_eq!(grade_from_result(true, true, 800.0), 4);
        assert_eq!(grade_from_result(true, true, 300.0), 5);
    }

    #[test]
    fn submit_creates_and_advances_card() {
        let s = Storage::open_in_memory();
        s.practice_submit_result("test", true, true, 300.0, 1_000_000);
        let stats = s.practice_card_stats("test");
        assert_eq!(stats.reps, 1);
        assert!(stats.due_at > 1_000_000, "due pushed into the future");
    }

    #[test]
    fn attempt_aggregates_compute() {
        let s = Storage::open_in_memory();
        let sid = s.practice_start_session(1_000_000);
        // clean fast correct
        s.practice_log_attempt(sid, "test", true, true, 300.0, 0, 0, false, 1_000_001);
        // messy correct with hint
        s.practice_log_attempt(sid, "test", true, false, 800.0, 4, 2, true, 1_000_002);
        let st = s.practice_card_stats("test");
        assert!((st.avg_backspaces - 2.0).abs() < 1e-9, "mean of 0 and 4");
        assert!((st.clean_rate - 0.5).abs() < 1e-9, "1 of 2 clean");
        assert!((st.best_fire_ms - 300.0).abs() < 1e-9, "min correct fire_ms");
        assert!((st.hint_rate - 0.5).abs() < 1e-9, "1 of 2 hinted");
    }

    #[test]
    fn session_summary_returns_attempts_in_ts_order() {
        let s = Storage::open_in_memory();
        let sid = s.practice_start_session(1_000_000);
        // Logged out of ts order to confirm the query sorts ascending.
        s.practice_log_attempt(sid, "beta", false, false, 800.0, 4, 2, true, 2_000);
        s.practice_log_attempt(sid, "alpha", true, true, 300.0, 0, 0, false, 1_000);
        let summary = s.practice_session_summary(sid);
        assert_eq!(summary.len(), 2);
        // ts ascending: alpha (1_000) then beta (2_000).
        assert_eq!(summary[0].phrase, "alpha");
        assert!(summary[0].correct);
        assert!(summary[0].first_try);
        assert!(!summary[0].hint_used);
        assert!((summary[0].fire_ms - 300.0).abs() < 1e-9);
        assert_eq!(summary[0].backspaces, 0);
        assert_eq!(summary[0].corrections, 0);
        assert_eq!(summary[0].ts, 1_000);
        assert_eq!(summary[1].phrase, "beta");
        assert!(!summary[1].correct);
        assert!(!summary[1].first_try);
        assert!(summary[1].hint_used);
        assert_eq!(summary[1].backspaces, 4);
        assert_eq!(summary[1].corrections, 2);
        assert_eq!(summary[1].ts, 2_000);
    }

    #[test]
    fn streak_counts_consecutive_days() {
        let s = Storage::open_in_memory();
        let day = 86_400_000i64;
        let now = 10 * day + 5_000; // some time on day 10
                                    // Completed sessions on days 10, 9, 8 -> streak 3; gap at 6.
        for d in [10i64, 9, 8, 6] {
            let id = s.practice_start_session(d * day);
            s.practice_complete_session(id, d * day + 100);
        }
        assert_eq!(s.practice_streak(now), 3);
    }
}
