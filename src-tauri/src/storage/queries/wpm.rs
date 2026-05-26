use crate::types::{WpmSample, WpmSummary};

use super::super::{active_ms, wpm_of, Storage, SESSION_GAP_MS};

impl Storage {
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
}
