//! Cron-lite schedule parsing for flow `[triggers] schedule` (spec
//! §10). A deliberately tiny grammar — enough for the common desktop
//! cases without a cron dependency — evaluated by the `tick` command
//! (driven by a systemd user timer). The trigger engine only ever
//! *starts flows*; it contains no LLM logic (the hot path stays
//! deterministic).
//!
//! Grammar (case-insensitive):
//!   "hourly"               — at the top of every hour
//!   "daily HH:MM"          — once a day at HH:MM local
//!   "weekly <dow> HH:MM"   — once a week (<dow> = mon..sun)
//!   "every <N>m"           — every N minutes
//!
//! Due-ness is decided against the flow's last run time (derived from
//! the latest run id `<flow>-<unix_millis>`), so a missed tick fires
//! on the next one rather than being skipped.

/// A parsed schedule spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schedule {
    Hourly,
    /// hour (0–23), minute (0–59)
    Daily(u32, u32),
    /// weekday (0=Mon … 6=Sun), hour, minute
    Weekly(u32, u32, u32),
    /// every N minutes
    EveryMinutes(u64),
}

impl Schedule {
    /// Parse one schedule string; `None` if it doesn't match the grammar.
    pub fn parse(s: &str) -> Option<Schedule> {
        let s = s.trim().to_ascii_lowercase();
        let parts: Vec<&str> = s.split_whitespace().collect();
        match parts.as_slice() {
            ["hourly"] => Some(Schedule::Hourly),
            ["daily", hm] => parse_hm(hm).map(|(h, m)| Schedule::Daily(h, m)),
            ["weekly", dow, hm] => {
                let d = parse_dow(dow)?;
                let (h, m) = parse_hm(hm)?;
                Some(Schedule::Weekly(d, h, m))
            }
            ["every", n] => {
                let mins = n.strip_suffix('m').unwrap_or(n).parse::<u64>().ok()?;
                (mins >= 1).then_some(Schedule::EveryMinutes(mins))
            }
            _ => None,
        }
    }

    /// Is this schedule due now, given when the flow last ran?
    /// `now` and `last_run` are unix seconds; `last_run = None` means it
    /// has never run (so any past-or-present occurrence is due).
    ///
    /// `local_offset_secs` is the local timezone offset from UTC (so
    /// callers pass the host's offset; tests pass 0). Day/week
    /// boundaries are computed in local time.
    pub fn is_due(&self, now: i64, last_run: Option<i64>, local_offset_secs: i64) -> bool {
        match self {
            Schedule::EveryMinutes(n) => match last_run {
                Some(last) => now - last >= (*n as i64) * 60,
                None => true,
            },
            Schedule::Hourly => {
                // The most recent top-of-hour occurrence (local).
                let occ = floor_to_hour(now + local_offset_secs) - local_offset_secs;
                last_run.map(|l| l < occ).unwrap_or(true) && now >= occ
            }
            Schedule::Daily(h, m) => {
                let occ = last_daily_occurrence(now, *h, *m, local_offset_secs);
                last_run.map(|l| l < occ).unwrap_or(true) && now >= occ
            }
            Schedule::Weekly(d, h, m) => {
                let occ = last_weekly_occurrence(now, *d, *h, *m, local_offset_secs);
                last_run.map(|l| l < occ).unwrap_or(true) && now >= occ
            }
        }
    }
}

fn parse_hm(hm: &str) -> Option<(u32, u32)> {
    let (h, m) = hm.split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    (h < 24 && m < 60).then_some((h, m))
}

fn parse_dow(d: &str) -> Option<u32> {
    Some(match &d[..d.len().min(3)] {
        "mon" => 0,
        "tue" => 1,
        "wed" => 2,
        "thu" => 3,
        "fri" => 4,
        "sat" => 5,
        "sun" => 6,
        _ => return None,
    })
}

const DAY: i64 = 86_400;

fn floor_to_hour(t: i64) -> i64 {
    t - t.rem_euclid(3600)
}

/// Local midnight (as a UTC unix-seconds instant) for the day
/// containing `now`.
fn local_midnight(now: i64, offset: i64) -> i64 {
    let local = now + offset;
    let midnight_local = local - local.rem_euclid(DAY);
    midnight_local - offset
}

/// Most recent occurrence of `h:m` local time at or before `now`.
fn last_daily_occurrence(now: i64, h: u32, m: u32, offset: i64) -> i64 {
    let today = local_midnight(now, offset) + (h as i64) * 3600 + (m as i64) * 60;
    if today <= now {
        today
    } else {
        today - DAY
    }
}

/// Local day-of-week for `now` (0=Mon … 6=Sun). 1970-01-01 was a
/// Thursday (=3).
fn local_dow(now: i64, offset: i64) -> u32 {
    let days = (now + offset).div_euclid(DAY);
    (((days % 7) + 3).rem_euclid(7)) as u32
}

/// Most recent occurrence of weekday `d` at `h:m` local, at or before now.
fn last_weekly_occurrence(now: i64, d: u32, h: u32, m: u32, offset: i64) -> i64 {
    let today_occ = local_midnight(now, offset) + (h as i64) * 3600 + (m as i64) * 60;
    let cur_dow = local_dow(now, offset) as i64;
    let mut back = (cur_dow - d as i64).rem_euclid(7);
    let mut occ = today_occ - back * DAY;
    if occ > now {
        back += 7;
        occ = today_occ - back * DAY;
    }
    occ
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_grammar() {
        assert_eq!(Schedule::parse("hourly"), Some(Schedule::Hourly));
        assert_eq!(Schedule::parse("daily 09:00"), Some(Schedule::Daily(9, 0)));
        assert_eq!(Schedule::parse("Daily 23:45"), Some(Schedule::Daily(23, 45)));
        assert_eq!(Schedule::parse("weekly mon 08:30"), Some(Schedule::Weekly(0, 8, 30)));
        assert_eq!(Schedule::parse("weekly Sunday 6:00"), Some(Schedule::Weekly(6, 6, 0)));
        assert_eq!(Schedule::parse("every 15m"), Some(Schedule::EveryMinutes(15)));
        assert_eq!(Schedule::parse("every 5"), Some(Schedule::EveryMinutes(5)));
        assert_eq!(Schedule::parse("daily 25:00"), None);
        assert_eq!(Schedule::parse("nonsense"), None);
    }

    #[test]
    fn every_minutes_due_after_interval() {
        let s = Schedule::EveryMinutes(10);
        assert!(s.is_due(1000, None, 0)); // never ran
        assert!(!s.is_due(1000, Some(700), 0)); // 300s < 600s
        assert!(s.is_due(1000, Some(300), 0)); // 700s >= 600s
    }

    #[test]
    fn daily_due_once_per_day_after_the_time() {
        // UTC (offset 0). Pick now = a known instant.
        // 2021-01-01 00:00:00 UTC = 1609459200. 09:00 that day = +9h.
        let nine_am = 1609459200 + 9 * 3600;
        let s = Schedule::Daily(9, 0);
        // Just after 09:00, never ran today → due.
        assert!(s.is_due(nine_am + 60, Some(nine_am - 3600), 0));
        // Already ran after 09:00 → not due again today.
        assert!(!s.is_due(nine_am + 120, Some(nine_am + 60), 0));
        // Before 09:00 (08:00), already ran at yesterday's 09:00 → not
        // due yet (today's 09:00 hasn't arrived).
        let eight_am = 1609459200 + 8 * 3600;
        assert!(!s.is_due(eight_am, Some(nine_am - DAY), 0));
        // …but if yesterday's 09:00 was MISSED (last ran 08:00), it's
        // due now to catch up.
        assert!(s.is_due(eight_am, Some(eight_am - DAY), 0));
    }

    #[test]
    fn weekly_lands_on_the_right_weekday() {
        // 1609459200 = 2021-01-01 = Friday (dow 4).
        let fri_midnight = 1609459200;
        let s = Schedule::Weekly(4, 0, 0); // Friday 00:00
        // At Friday 00:01, never ran → due.
        assert!(s.is_due(fri_midnight + 60, None, 0));
        // Saturday: last weekly occurrence was Friday; if ran Friday, not due.
        let sat = fri_midnight + DAY;
        assert!(!s.is_due(sat, Some(fri_midnight + 60), 0));
    }
}
