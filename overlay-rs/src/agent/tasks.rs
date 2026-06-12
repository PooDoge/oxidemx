//! Backend for the `schedule_task` agent tool: systemd **user**
//! timers, one `.service` + `.timer` pair per task, written to
//! `~/.config/systemd/user/oxidemx-task-<slug>.{service,timer}`.
//!
//! systemd is deliberately the only scheduler here — no in-process
//! cron clone. Timers survive overlay restarts and logouts
//! (`Persistent=true` catches missed runs), `systemctl --user`
//! gives the user full visibility outside our UI, and deleting the
//! two files removes every trace.
//!
//! File generation and parsing are parameterised by directory
//! (`*_in` variants) so they're unit-testable without a systemd
//! instance; the public functions bind the real unit dir and shell
//! out to `systemctl --user`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Prefix shared by every unit this module owns. Used both to name
/// new units and to recognise ours when listing.
const UNIT_PREFIX: &str = "oxidemx-task-";

/// One scheduled task, as surfaced to the model (JSON via serde) and
/// to the UI cards.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskInfo {
    /// Unit base name ("oxidemx-task-<slug>"), without the
    /// `.service`/`.timer` suffix. This is the handle every other
    /// function in this module takes.
    pub unit: String,
    /// Human-readable name, stored as the units' `Description=`.
    pub name: String,
    /// The timer's `OnCalendar=` expression, verbatim.
    pub schedule: String,
    /// Human relative next-fire time ("in 6h 28m") from
    /// `list-timers`; `None` when disabled or not scheduled.
    pub next_run: Option<String>,
    pub enabled: bool,
}

// =============================================================================
// PURE HELPERS (unit-testable, no systemctl)
// =============================================================================

/// Turn a human task name into a systemd-safe unit slug: lowercase
/// `[a-z0-9-]`, runs of anything else collapsed to a single dash,
/// dashes trimmed from the ends. Never empty — a name with no usable
/// characters falls back to "task".
pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut pending_dash = false;
    for c in name.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(c);
        } else {
            // Any separator/punctuation/unicode → at most one dash,
            // and only between alphanumeric runs (trims the ends).
            pending_dash = true;
        }
    }
    if out.is_empty() {
        "task".to_string()
    } else {
        out
    }
}

/// Escape a value for embedding inside a single-quoted systemd
/// `ExecStart=` argument. systemd's unit-file lexer accepts C-style
/// escapes inside quotes, so `\'` and `\\` are honoured; `%` is a
/// specifier introducer everywhere in the line and must be doubled.
fn escape_for_unit(command: &str) -> String {
    command
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('%', "%%")
}

/// Render the `.service` unit body for a task.
fn service_body(name: &str, command: &str) -> String {
    format!(
        "[Unit]\n\
         Description={name}\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart=/bin/sh -c '{}'\n",
        escape_for_unit(command)
    )
}

/// Render the `.timer` unit body for a task. `Persistent=true` makes
/// systemd fire a missed schedule at the next boot/login instead of
/// silently skipping it.
fn timer_body(name: &str, unit: &str, on_calendar: &str) -> String {
    format!(
        "[Unit]\n\
         Description={name}\n\
         \n\
         [Timer]\n\
         OnCalendar={on_calendar}\n\
         Persistent=true\n\
         Unit={unit}.service\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n"
    )
}

/// Write the `.service`/`.timer` pair into `dir` and return the
/// (not-yet-enabled) `TaskInfo`. Pure file I/O — `create` layers the
/// `systemctl` calls on top.
fn create_in(dir: &Path, name: &str, on_calendar: &str, command: &str) -> Result<TaskInfo, String> {
    let unit = format!("{UNIT_PREFIX}{}", slugify(name));
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;

    let service_path = dir.join(format!("{unit}.service"));
    let timer_path = dir.join(format!("{unit}.timer"));
    std::fs::write(&service_path, service_body(name, command))
        .map_err(|e| format!("writing {}: {e}", service_path.display()))?;
    std::fs::write(&timer_path, timer_body(name, &unit, on_calendar))
        .map_err(|e| format!("writing {}: {e}", timer_path.display()))?;

    Ok(TaskInfo {
        unit,
        name: name.to_string(),
        schedule: on_calendar.to_string(),
        next_run: None,
        enabled: false,
    })
}

/// Pull `Description=` and `OnCalendar=` back out of a `.timer` file
/// we wrote. Tolerant of hand edits — missing fields degrade to the
/// slug/empty string rather than erroring.
fn parse_timer_file(unit: &str, content: &str) -> TaskInfo {
    let mut name = String::new();
    let mut schedule = String::new();
    for line in content.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("Description=") {
            if name.is_empty() {
                name = v.to_string();
            }
        } else if let Some(v) = line.strip_prefix("OnCalendar=") {
            if schedule.is_empty() {
                schedule = v.to_string();
            }
        }
    }
    if name.is_empty() {
        // Fall back to the slug part of the unit name.
        name = unit.strip_prefix(UNIT_PREFIX).unwrap_or(unit).to_string();
    }
    TaskInfo {
        unit: unit.to_string(),
        name,
        schedule,
        next_run: None,
        enabled: false,
    }
}

/// Enumerate `oxidemx-task-*.timer` files in `dir` and parse each
/// into a `TaskInfo` (enabled/next_run left at their defaults —
/// `list` fills those from systemctl). Sorted by unit name so the
/// listing is stable.
fn list_in(dir: &Path) -> Vec<TaskInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut tasks = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        let Some(unit) = file_name
            .strip_suffix(".timer")
            .filter(|base| base.starts_with(UNIT_PREFIX))
        else {
            continue;
        };
        let content = std::fs::read_to_string(entry.path()).unwrap_or_default();
        tasks.push(parse_timer_file(unit, &content));
    }
    tasks.sort_by(|a, b| a.unit.cmp(&b.unit));
    tasks
}

/// Render a microseconds-since-epoch timestamp as a relative
/// "in 6h 28m" string against `now_secs`. Past/immediate → "now".
fn format_relative(next_usec: u64, now_secs: u64) -> String {
    let next_secs = next_usec / 1_000_000;
    if next_secs <= now_secs {
        return "now".to_string();
    }
    let delta = next_secs - now_secs;
    let (days, hours, mins) = (
        delta / 86_400,
        (delta % 86_400) / 3_600,
        (delta % 3_600) / 60,
    );
    if days > 0 {
        format!("in {days}d {hours}h")
    } else if hours > 0 {
        format!("in {hours}h {mins}m")
    } else if mins > 0 {
        format!("in {mins}m")
    } else {
        format!("in {delta}s")
    }
}

/// Parse `systemctl --user list-timers --all --output=json` output
/// into `(timer unit file name → relative next-run)` pairs. "next"
/// is a realtime microseconds timestamp, or null/absent when the
/// timer has nothing scheduled.
fn parse_list_timers(json: &str, now_secs: u64) -> Vec<(String, String)> {
    let Ok(rows) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for row in rows.as_array().into_iter().flatten() {
        let Some(unit) = row["unit"].as_str() else {
            continue;
        };
        if let Some(next) = row["next"].as_u64() {
            out.push((unit.to_string(), format_relative(next, now_secs)));
        }
    }
    out
}

/// Count `oxidemx-task-*` timers whose next run falls within
/// `window_secs` of `now_secs`. Pure half of `due_within_24h_count`.
fn due_in_window(json: &str, now_secs: u64, window_secs: u64) -> u32 {
    let Ok(rows) = serde_json::from_str::<serde_json::Value>(json) else {
        return 0;
    };
    let mut count = 0;
    for row in rows.as_array().into_iter().flatten() {
        let Some(unit) = row["unit"].as_str() else {
            continue;
        };
        if !unit.starts_with("oxidemx-task-") {
            continue;
        }
        if let Some(next_usec) = row["next"].as_u64() {
            let next_secs = next_usec / 1_000_000;
            if next_secs >= now_secs && next_secs <= now_secs + window_secs {
                count += 1;
            }
        }
    }
    count
}

/// Number of scheduled OxideMX tasks due within the next 24 hours —
/// feeds the Splice Widgets "Tasks" wedge.
pub fn due_within_24h_count() -> u32 {
    match systemctl(&["list-timers", "--all", "--output=json"]) {
        Ok(json) => due_in_window(&json, unix_now(), 86_400),
        Err(_) => 0,
    }
}

// =============================================================================
// SYSTEMCTL-BACKED PUBLIC API
// =============================================================================

/// `~/.config/systemd/user` — where systemd looks for user units.
fn user_unit_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/jim".to_string());
    Path::new(&home).join(".config/systemd/user")
}

/// Run `systemctl --user <args>`, returning stdout or the failure's
/// stderr as the error string.
fn systemctl(args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .map_err(|e| format!("failed to run systemctl: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!(
            "systemctl --user {} failed: {}",
            args.join(" "),
            err.trim()
        ))
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Look up the relative next-run string for `<unit>.timer`.
fn next_run_for(unit: &str) -> Option<String> {
    let json = systemctl(&["list-timers", "--all", "--output=json"]).ok()?;
    let timer_name = format!("{unit}.timer");
    parse_list_timers(&json, unix_now())
        .into_iter()
        .find(|(u, _)| *u == timer_name)
        .map(|(_, next)| next)
}

/// Create a task: write the unit pair, `daemon-reload`, then
/// `enable --now` the timer so it's live immediately.
pub fn create(name: &str, on_calendar: &str, command: &str) -> Result<TaskInfo, String> {
    let mut info = create_in(&user_unit_dir(), name, on_calendar, command)?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", &format!("{}.timer", info.unit)])?;
    info.enabled = true;
    info.next_run = next_run_for(&info.unit);
    Ok(info)
}

/// Enable (`enable --now`) or disable (`disable --now`) a task's
/// timer. `unit` is the base name from [`TaskInfo::unit`].
pub fn set_enabled(unit: &str, enabled: bool) -> Result<(), String> {
    let timer = format!("{unit}.timer");
    let action = if enabled { "enable" } else { "disable" };
    systemctl(&[action, "--now", &timer])?;
    Ok(())
}

/// Fire the task's service right now, independent of its schedule.
pub fn run_now(unit: &str) -> Result<(), String> {
    systemctl(&["start", &format!("{unit}.service")])?;
    Ok(())
}

/// Remove a task entirely: disable the timer, delete both unit
/// files, reload. Missing files are ignored so a half-deleted task
/// can always be cleaned up by deleting again.
pub fn delete(unit: &str) -> Result<(), String> {
    // Best-effort disable — the timer may already be disabled or
    // half-removed, which must not block file deletion.
    let _ = systemctl(&["disable", "--now", &format!("{unit}.timer")]);
    let dir = user_unit_dir();
    for suffix in ["service", "timer"] {
        let path = dir.join(format!("{unit}.{suffix}"));
        if let Err(e) = std::fs::remove_file(&path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(format!("removing {}: {e}", path.display()));
            }
        }
    }
    systemctl(&["daemon-reload"])?;
    Ok(())
}

/// All OxideMX tasks: unit files parsed from disk, `enabled` from
/// `is-enabled`, `next_run` from `list-timers`. systemctl failures
/// degrade to disabled/None rather than hiding the task.
pub fn list() -> Vec<TaskInfo> {
    let mut tasks = list_in(&user_unit_dir());
    let next_runs = systemctl(&["list-timers", "--all", "--output=json"])
        .map(|json| parse_list_timers(&json, unix_now()))
        .unwrap_or_default();
    for task in &mut tasks {
        // `is-enabled` exits non-zero for "disabled", so the Err arm
        // is the normal disabled path, not just a failure path.
        task.enabled = systemctl(&["is-enabled", &format!("{}.timer", task.unit)])
            .map(|s| s.trim() == "enabled")
            .unwrap_or(false);
        let timer_name = format!("{}.timer", task.unit);
        task.next_run = next_runs
            .iter()
            .find(|(u, _)| *u == timer_name)
            .map(|(_, next)| next.clone());
    }
    tasks
}

#[cfg(test)]
mod tests {
    #[test]
    fn due_in_window_counts_only_oxidemx_timers_in_range() {
        let now = 1_000_000u64; // secs
        let json = format!(
            r#"[
              {{"unit":"oxidemx-task-a.timer","next":{}}},
              {{"unit":"oxidemx-task-b.timer","next":{}}},
              {{"unit":"oxidemx-task-c.timer","next":null}},
              {{"unit":"systemd-tmpfiles-clean.timer","next":{}}}
            ]"#,
            (now + 3_600) * 1_000_000,  // in 1h → counts
            (now + 90_000) * 1_000_000, // in 25h → outside window
            (now + 60) * 1_000_000,     // foreign unit → ignored
        );
        assert_eq!(super::due_in_window(&json, now, 86_400), 1);
    }

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oxidemx-tasks-test-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn slugify_basics() {
        assert_eq!(slugify("Backup Photos"), "backup-photos");
        assert_eq!(slugify("  weird---name!!  "), "weird-name");
        assert_eq!(slugify("UPPER_case.task"), "upper-case-task");
        assert_eq!(slugify("héllo wörld"), "h-llo-w-rld");
        assert_eq!(slugify("---"), "task");
        assert_eq!(slugify(""), "task");
    }

    #[test]
    fn create_in_writes_both_units_with_expected_lines() {
        let dir = temp_dir("create");
        let info = create_in(
            &dir,
            "Nightly Backup",
            "*-*-* 03:00:00",
            "rsync -a ~/docs /backup",
        )
        .unwrap();
        assert_eq!(info.unit, "oxidemx-task-nightly-backup");
        assert_eq!(info.name, "Nightly Backup");
        assert_eq!(info.schedule, "*-*-* 03:00:00");

        let service =
            std::fs::read_to_string(dir.join("oxidemx-task-nightly-backup.service")).unwrap();
        assert!(service.contains("Description=Nightly Backup"));
        assert!(service.contains("Type=oneshot"));
        assert!(service.contains("ExecStart=/bin/sh -c 'rsync -a ~/docs /backup'"));

        let timer = std::fs::read_to_string(dir.join("oxidemx-task-nightly-backup.timer")).unwrap();
        assert!(timer.contains("Description=Nightly Backup"));
        assert!(timer.contains("OnCalendar=*-*-* 03:00:00"));
        assert!(timer.contains("Persistent=true"));
        assert!(timer.contains("Unit=oxidemx-task-nightly-backup.service"));
        assert!(timer.contains("WantedBy=timers.target"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exec_start_escapes_quotes_and_specifiers() {
        let body = service_body("t", "echo 'hi' && date +%H");
        assert!(body.contains(r"ExecStart=/bin/sh -c 'echo \'hi\' && date +%%H'"));
    }

    #[test]
    fn list_in_parses_synthetic_dir_and_ignores_foreign_units() {
        let dir = temp_dir("list");
        create_in(&dir, "Task B", "daily", "true").unwrap();
        create_in(&dir, "Task A", "hourly", "true").unwrap();
        // Foreign unit + non-timer files must be skipped.
        std::fs::write(dir.join("someone-elses.timer"), "[Timer]\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "hi").unwrap();

        let tasks = list_in(&dir);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].unit, "oxidemx-task-task-a");
        assert_eq!(tasks[0].name, "Task A");
        assert_eq!(tasks[0].schedule, "hourly");
        assert_eq!(tasks[1].unit, "oxidemx-task-task-b");
        assert!(!tasks[0].enabled);
        assert_eq!(tasks[0].next_run, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_timers_json_parses_next_and_skips_null() {
        let now = 1_000_000u64; // seconds
        let json = format!(
            r#"[
                {{"next": {}, "unit": "oxidemx-task-a.timer", "activates": "oxidemx-task-a.service"}},
                {{"next": null, "unit": "oxidemx-task-b.timer", "activates": "oxidemx-task-b.service"}}
            ]"#,
            (now + 6 * 3600 + 28 * 60) * 1_000_000
        );
        let parsed = parse_list_timers(&json, now);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, "oxidemx-task-a.timer");
        assert_eq!(parsed[0].1, "in 6h 28m");
    }

    #[test]
    fn format_relative_buckets() {
        let now = 10_000u64;
        let usec = |s: u64| (now + s) * 1_000_000;
        assert_eq!(format_relative(usec(30), now), "in 30s");
        assert_eq!(format_relative(usec(5 * 60), now), "in 5m");
        assert_eq!(format_relative(usec(2 * 3600 + 90), now), "in 2h 1m");
        assert_eq!(format_relative(usec(3 * 86400 + 4 * 3600), now), "in 3d 4h");
        assert_eq!(format_relative(now * 1_000_000, now), "now");
    }
}
