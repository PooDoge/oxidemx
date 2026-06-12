//! Backend for the `execute_command` agent tool.
//!
//! Policy: a command runs without confirmation only when its leading
//! tokens match an allowlist entry exactly (whole tokens, never
//! substrings — "brightnessctlx" must not ride on "brightnessctl",
//! and "systemctl enable" must not ride on "systemctl --user").
//! Anything off-list goes through the chat's confirmation chip; the
//! caller in `ai_client.rs` owns that interaction and only calls
//! [`run`] once the user has approved.

use std::process::Stdio;
use std::time::Duration;

/// Wall-clock budget for one command. The agent loop blocks on the
/// result, so a hung command would otherwise freeze the whole turn.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Cap on the output we feed back to the model / render in the card.
/// 2 KiB is plenty for status-style commands and keeps a `journalctl`
/// accident from blowing up the context window.
const OUTPUT_CAP: usize = 2048;

/// Conventional "killed by timeout" exit code (matches coreutils
/// `timeout(1)`), so the model can recognise the condition.
const TIMEOUT_EXIT_CODE: i32 = 124;

/// Whole-token prefix match of `command` against `allowlist`.
///
/// An entry like `"systemctl --user"` matches any command whose
/// first N whitespace-split tokens equal the entry's N tokens:
/// `systemctl --user enable foo.timer` passes, `systemctl enable
/// foo.timer` does not. Token equality (not `starts_with` on the
/// string) is what stops `brightnessctlx …` from matching a
/// `brightnessctl` entry.
pub fn is_allowlisted(command: &str, allowlist: &[String]) -> bool {
    let cmd_tokens: Vec<&str> = command.split_whitespace().collect();
    allowlist.iter().any(|entry| {
        let entry_tokens: Vec<&str> = entry.split_whitespace().collect();
        !entry_tokens.is_empty()
            && cmd_tokens.len() >= entry_tokens.len()
            && cmd_tokens[..entry_tokens.len()] == entry_tokens[..]
    })
}

/// The user's configured allowlist, freshly loaded from the main
/// config file so mid-session edits take effect on the next call.
/// Falls back to the built-in default list when the config is
/// missing or unreadable (same behaviour as the rest of the app).
pub fn allowlist() -> Vec<String> {
    oxidemx_shared::config::default_config_path()
        .and_then(|p| oxidemx_shared::AppConfig::load_from(&p).ok())
        .map(|c| c.overlay.ai.command_allowlist)
        .unwrap_or_else(|| oxidemx_shared::config::AiConfig::default().command_allowlist)
}

/// Run `command` via `sh -c` and return `(output, exit_code)`.
///
/// stdout and stderr are captured separately and concatenated
/// (stderr appended) so error text survives even when a command
/// writes its diagnostics to stderr only. Output is capped at
/// [`OUTPUT_CAP`] chars with a trailing `…` marker. A command that
/// outlives [`TIMEOUT`] is killed (`kill_on_drop`) and reported as
/// exit code 124 with a note. Never panics — spawn failures come
/// back as `(message, -1)`.
pub async fn run(command: &str) -> (String, i32) {
    let child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output();

    match tokio::time::timeout(TIMEOUT, child).await {
        Ok(Ok(output)) => {
            let mut text = String::from_utf8_lossy(&output.stdout)
                .trim_end()
                .to_string();
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr.trim_end();
            if !stderr.is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(stderr);
            }
            // `code()` is None when the process died from a signal;
            // -1 is our "abnormal exit" marker for that case.
            let code = output.status.code().unwrap_or(-1);
            (cap_output(text), code)
        }
        Ok(Err(e)) => (format!("Failed to spawn shell: {e}"), -1),
        Err(_) => (
            format!(
                "Command timed out after {} s and was killed.",
                TIMEOUT.as_secs()
            ),
            TIMEOUT_EXIT_CODE,
        ),
    }
}

/// Truncate to [`OUTPUT_CAP`] bytes on a char boundary, appending an
/// ellipsis when anything was dropped.
fn cap_output(mut s: String) -> String {
    if s.len() > OUTPUT_CAP {
        let mut cut = OUTPUT_CAP;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push('…');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn single_token_entry_matches_command_and_args() {
        let al = list(&["brightnessctl"]);
        assert!(is_allowlisted("brightnessctl", &al));
        assert!(is_allowlisted("brightnessctl set 50%", &al));
    }

    #[test]
    fn entry_must_match_whole_tokens_not_substrings() {
        let al = list(&["brightnessctl"]);
        assert!(!is_allowlisted("brightnessctlx set 50%", &al));
        assert!(!is_allowlisted("xbrightnessctl", &al));
    }

    #[test]
    fn multi_token_entry_requires_all_tokens_in_order() {
        let al = list(&["systemctl --user"]);
        assert!(is_allowlisted("systemctl --user enable foo.timer", &al));
        assert!(is_allowlisted("systemctl --user status", &al));
        assert!(!is_allowlisted("systemctl enable foo.timer", &al));
        assert!(!is_allowlisted("systemctl", &al));
    }

    #[test]
    fn empty_allowlist_matches_nothing() {
        assert!(!is_allowlisted("ls", &[]));
    }

    #[test]
    fn extra_whitespace_is_normalised_on_both_sides() {
        let al = list(&["systemctl   --user", "  wpctl "]);
        assert!(is_allowlisted("systemctl  --user   stop foo", &al));
        assert!(is_allowlisted("wpctl set-volume @DEFAULT_SINK@ 5%+", &al));
    }

    #[test]
    fn empty_or_blank_entries_never_match() {
        let al = list(&["", "   "]);
        assert!(!is_allowlisted("ls", &al));
        assert!(!is_allowlisted("", &al));
    }

    #[test]
    fn cap_output_truncates_on_char_boundary() {
        let s = "é".repeat(OUTPUT_CAP); // 2 bytes per char
        let capped = cap_output(s);
        assert!(capped.ends_with('…'));
        assert!(capped.len() <= OUTPUT_CAP + '…'.len_utf8());
        // Short output passes through untouched.
        assert_eq!(cap_output("ok".into()), "ok");
    }

    #[tokio::test]
    async fn run_captures_stdout_stderr_and_exit_code() {
        let (out, code) = run("echo hi; echo err >&2; exit 3").await;
        assert!(out.contains("hi"));
        assert!(out.contains("err"));
        assert_eq!(code, 3);
    }

    #[tokio::test]
    async fn run_never_panics_on_garbage() {
        let (_out, code) = run("definitely-not-a-real-binary-xyz").await;
        assert_ne!(code, 0);
    }
}
