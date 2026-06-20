//! Shell-command safety classifier.
//!
//! Classifies a raw shell command string into a [`Decision`] without executing
//! anything. The classification is conservative: anything not provably safe
//! returns [`Tier::Ask`].

use crate::{Decision, Tier};

/// Shell metacharacters that indicate a compound or redirected command.
/// We check for these BEFORE attempting to split the command.
const METACHARACTERS: &[char] = &[';', '|', '`', '>', '<', '&', '\n'];

/// Leading `NAME=value ` environment assignment pattern.
fn has_leading_env_assignment(cmd: &str) -> bool {
    // Matches `IDENT=...` at the start (before any whitespace-separated word).
    let first = cmd.split_whitespace().next().unwrap_or("");
    // Must contain `=` but not start with `-` (flags) or `/` (paths).
    if first.starts_with('-') || first.starts_with('/') {
        return false;
    }
    if let Some(eq_pos) = first.find('=') {
        // The part before `=` must be a valid env-var name: ASCII alphanum + `_`,
        // starting with a letter or `_`.
        let name = &first[..eq_pos];
        if name.is_empty() {
            return false;
        }
        let mut chars = name.chars();
        let first_char = chars.next().unwrap();
        (first_char.is_ascii_alphabetic() || first_char == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    } else {
        false
    }
}

/// Hard-denied executables (privilege escalation / network exfil / destructive).
const ALWAYS_DENY_ARGV0: &[&str] = &[
    "sudo", "su", "pkexec", "curl", "wget", "nc", "ncat", "ssh", "scp", "dd", "mkfs",
];

/// Read-only bare programs (single-word, no subcommand needed).
const READONLY_BARE: &[&str] = &[
    "ls", "cat", "rg", "grep", "pwd", "echo", "head", "tail", "wc", "find",
];

/// Read-only `git` subcommands (only allowed if all dashed flags are safe).
const GIT_READONLY_SUB: &[&str] = &["diff", "status", "log", "show"];

/// Safe dashed flags for read-only git subcommands (diff/log/show/status).
/// Any other dashed flag → Ask.
const GIT_READONLY_SAFE_FLAGS: &[&str] = &[
    "--stat",
    "--cached",
    "--staged",
    "--name-only",
    "--name-status",
    "--oneline",
    "--graph",
    "-p",
    "--patch",
    "-v",
    "--list",
    "-a",
    "-r",
    "--all",
    "--short",
    "--summary",
    "--",
];

/// Safe dashed flags for `cargo` read-only subcommands.
const CARGO_READONLY_SAFE_FLAGS: &[&str] = &[
    "--release",
    "--all-features",
    "--no-default-features",
    "--workspace",
    "--all",
    "--tests",
    "--benches",
    "--examples",
    "--lib",
    "--bins",
    "--verbose",
    "-v",
    "--quiet",
    "-q",
    "--",
];

/// Read-only `cargo` subcommands.
const CARGO_READONLY_SUB: &[&str] = &["check", "test", "build", "clippy", "fmt", "tree"];

/// Returns `true` if a dashed flag token is safe for git read-only subcommands.
///
/// A bare `-<n>` numeric short form (e.g. `-5`, `-10`) is also safe (used by
/// `git log` to limit output to N commits).
fn git_flag_is_safe(flag: &str) -> bool {
    if GIT_READONLY_SAFE_FLAGS.contains(&flag) {
        return true;
    }
    // `-<digits>` numeric short-form, e.g. `-5`
    if flag.len() >= 2 && flag.starts_with('-') && !flag.starts_with("--") {
        let rest = &flag[1..];
        if rest.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    false
}

/// Returns `true` if a dashed flag token is safe for cargo read-only subcommands.
///
/// `-p <pkg>` and `--package <pkg>` are safe (value follows as a separate
/// token — we only check the flag itself, not its value).
fn cargo_flag_is_safe(flag: &str) -> bool {
    if CARGO_READONLY_SAFE_FLAGS.contains(&flag) {
        return true;
    }
    // `-p` / `--package` — package selector
    if flag == "-p" || flag == "--package" {
        return true;
    }
    false
}

/// Classify a raw shell command string.
///
/// Decision ladder:
///
/// 1. Shell metacharacter or leading env assignment → [`Tier::Ask`]
/// 2. Parse with `shell-words` (on error → [`Tier::Ask`])
/// 3. Hard-deny rules → [`Tier::AutoDeny`]
/// 4. Known read-only allowlist (with flag validation) → [`Tier::AutoAllow`]
/// 5. Everything else → [`Tier::Ask`]
pub fn classify_command(cmd: &str) -> Decision {
    // ── Step 1: metacharacter / compound command check ────────────────────────
    // Also check for `$(` and `${` subshell expansions.
    if METACHARACTERS.iter().any(|&c| cmd.contains(c))
        || cmd.contains("$(")
        || cmd.contains("${")
        || has_leading_env_assignment(cmd)
    {
        return Decision {
            tier: Tier::Ask,
            reason: "compound/redirected command needs review".into(),
        };
    }

    // ── Step 2: parse into argv ───────────────────────────────────────────────
    let argv = match shell_words::split(cmd) {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => {
            return Decision {
                tier: Tier::Ask,
                reason: "empty command".into(),
            }
        }
        Err(_) => {
            return Decision {
                tier: Tier::Ask,
                reason: "could not parse command — review manually".into(),
            }
        }
    };

    let prog = argv[0].as_str();
    let sub = argv.get(1).map(String::as_str).unwrap_or("");

    // ── Step 3: hard deny ─────────────────────────────────────────────────────
    if ALWAYS_DENY_ARGV0.contains(&prog) {
        return Decision {
            tier: Tier::AutoDeny,
            reason: format!(
                "`{prog}` is in the hard-deny list (privilege escalation / network / destructive)"
            ),
        };
    }

    // `rm` with recursive flag (case-insensitive on the flag characters) or
    // an absolute path arg.
    if prog == "rm" {
        let has_recursive = argv[1..].iter().any(|a| {
            let lower = a.to_lowercase();
            matches!(
                lower.as_str(),
                "-rf" | "-fr" | "-r" | "--recursive" | "-rf/" | "-fr/"
            ) || (a.starts_with('-') && lower.contains('r') && lower.contains('f'))
            // catch -R, -Rf, -RF, -rF, etc.
            || (a.starts_with('-') && !a.starts_with("--") && lower.contains('r'))
        });
        let has_abs = argv[1..].iter().any(|a| a.starts_with('/'));
        if has_recursive || has_abs {
            return Decision {
                tier: Tier::AutoDeny,
                reason: "`rm` with recursive flag or absolute path is too dangerous to auto-run"
                    .into(),
            };
        }
    }

    // `git push --force` / `--force-with-lease`
    if prog == "git" && sub == "push" {
        let force = argv[2..].iter().any(|a| {
            matches!(a.as_str(), "--force" | "-f" | "--force-with-lease")
        });
        if force {
            return Decision {
                tier: Tier::AutoDeny,
                reason: "`git push --force` can overwrite remote history".into(),
            };
        }
    }

    // `git clean` — discards untracked files
    if prog == "git" && sub == "clean" {
        return Decision {
            tier: Tier::AutoDeny,
            reason: "`git clean` irreversibly removes untracked files".into(),
        };
    }

    // `git reset --hard`
    if prog == "git" && sub == "reset" {
        let hard = argv[2..].iter().any(|a| a == "--hard");
        if hard {
            return Decision {
                tier: Tier::AutoDeny,
                reason: "`git reset --hard` discards uncommitted changes".into(),
            };
        }
    }

    // ── Step 4: known-safe allowlist ──────────────────────────────────────────
    if READONLY_BARE.contains(&prog) {
        return Decision {
            tier: Tier::AutoAllow,
            reason: format!("`{prog}` is a read-only utility"),
        };
    }

    // git read-only subcommands: diff, status, log, show — allow only if all
    // dashed flags (tokens starting with `-`) are in the safe-flag allowlist.
    if prog == "git" && GIT_READONLY_SUB.contains(&sub) {
        let unsafe_flag = argv[2..].iter().find(|a| {
            a.starts_with('-') && !git_flag_is_safe(a.as_str())
        });
        if let Some(bad) = unsafe_flag {
            return Decision {
                tier: Tier::Ask,
                reason: format!(
                    "`git {sub}` with unrecognized flag `{bad}` — review"
                ),
            };
        }
        return Decision {
            tier: Tier::AutoAllow,
            reason: format!("`git {sub}` is a read-only git operation"),
        };
    }

    // `git branch` — allow only bare listing forms; any mutation flag → Ask.
    if prog == "git" && sub == "branch" {
        // Mutation flags: -D, -d, -m, -M, -c, -C, --delete, --move, --copy,
        // --force, and compound short forms like -vD, -Dv, etc.
        let mutation_flag = argv[2..].iter().any(|a| {
            if !a.starts_with('-') {
                return false;
            }
            if a.starts_with("--") {
                return matches!(
                    a.as_str(),
                    "--delete" | "--move" | "--copy" | "--force" | "--set-upstream-to"
                        | "--unset-upstream" | "--edit-description"
                );
            }
            // Short flags: reject if any character is a mutation indicator.
            // Allowed short chars: v, a, r (list-related).
            let inner = &a[1..]; // strip leading `-`
            inner.chars().any(|c| matches!(c, 'D' | 'd' | 'm' | 'M' | 'c' | 'C'))
        });
        if mutation_flag {
            return Decision {
                tier: Tier::Ask,
                reason: "`git branch` with mutation flag — review required".into(),
            };
        }
        // Also apply the safe-flag gate (unknown flags like --output, etc.).
        let unsafe_flag = argv[2..].iter().find(|a| {
            a.starts_with('-') && !git_flag_is_safe(a.as_str())
        });
        if let Some(bad) = unsafe_flag {
            return Decision {
                tier: Tier::Ask,
                reason: format!(
                    "`git branch` with unrecognized flag `{bad}` — review"
                ),
            };
        }
        return Decision {
            tier: Tier::AutoAllow,
            reason: "`git branch` listing is read-only".into(),
        };
    }

    // `git remote -v` — two-word subcommand
    if prog == "git" && sub == "remote" {
        let next = argv.get(2).map(String::as_str).unwrap_or("");
        if next == "-v" || argv.len() == 2 {
            return Decision {
                tier: Tier::AutoAllow,
                reason: "`git remote` / `git remote -v` is read-only".into(),
            };
        }
        // Any other form of `git remote` (add/remove/set-url…) → Ask.
        return Decision {
            tier: Tier::Ask,
            reason: "`git remote` subcommand may mutate config — review required".into(),
        };
    }

    // cargo read-only subcommands: allow only if all dashed flags are in the
    // safe-flag allowlist. Unknown flags → Ask.
    if prog == "cargo" && CARGO_READONLY_SUB.contains(&sub) {
        // `cargo fmt` mutates files but results are git-restorable.
        // Reason string reflects this; still AutoAllow per spec §5.1.
        let unsafe_flag = argv[2..].iter().find(|a| {
            a.starts_with('-') && !cargo_flag_is_safe(a.as_str())
        });
        if let Some(bad) = unsafe_flag {
            return Decision {
                tier: Tier::Ask,
                reason: format!(
                    "`cargo {sub}` with unrecognized flag `{bad}` — review"
                ),
            };
        }
        let reason = if sub == "fmt" {
            format!("`cargo {sub}` is a build/format command, results git-restorable")
        } else {
            format!("`cargo {sub}` is a read-only or build-only cargo subcommand")
        };
        return Decision {
            tier: Tier::AutoAllow,
            reason,
        };
    }

    // ── Step 5: conservative fallback ────────────────────────────────────────
    Decision {
        tier: Tier::Ask,
        reason: format!("`{prog}` is not on the auto-allow list — review required"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    // ── original tests (must stay passing) ───────────────────────────────────

    #[test]
    fn sudo_auto_deny() {
        assert_eq!(classify_command("sudo apt update").tier, Tier::AutoDeny);
    }

    #[test]
    fn git_log_auto_allow() {
        assert_eq!(classify_command("git log --oneline").tier, Tier::AutoAllow);
    }

    #[test]
    fn rm_rf_auto_deny() {
        assert_eq!(classify_command("rm -rf target/").tier, Tier::AutoDeny);
    }

    #[test]
    fn unknown_program_ask() {
        assert_eq!(classify_command("make install").tier, Tier::Ask);
    }

    // ── Fix 1: git read-only flag gate ────────────────────────────────────────

    #[test]
    fn git_show_output_flag_ask() {
        // --output= can write to arbitrary files — must be Ask.
        assert_eq!(
            classify_command("git show --output=/tmp/x").tier,
            Tier::Ask
        );
    }

    #[test]
    fn git_branch_delete_ask() {
        assert_eq!(classify_command("git branch -D foo").tier, Tier::Ask);
    }

    #[test]
    fn git_branch_lowercase_d_ask() {
        assert_eq!(classify_command("git branch -d foo").tier, Tier::Ask);
    }

    #[test]
    fn git_branch_v_auto_allow() {
        assert_eq!(classify_command("git branch -v").tier, Tier::AutoAllow);
    }

    #[test]
    fn git_branch_bare_auto_allow() {
        assert_eq!(classify_command("git branch").tier, Tier::AutoAllow);
    }

    #[test]
    fn git_diff_stat_auto_allow() {
        assert_eq!(classify_command("git diff --stat").tier, Tier::AutoAllow);
    }

    #[test]
    fn git_log_numeric_auto_allow() {
        // `-5` is a valid numeric limit for git log.
        assert_eq!(classify_command("git log -5").tier, Tier::AutoAllow);
    }

    #[test]
    fn git_log_unknown_flag_ask() {
        assert_eq!(classify_command("git log --unknown-flag").tier, Tier::Ask);
    }

    // ── Fix 1: cargo flag gate ────────────────────────────────────────────────

    #[test]
    fn cargo_test_release_auto_allow() {
        assert_eq!(classify_command("cargo test --release").tier, Tier::AutoAllow);
    }

    #[test]
    fn cargo_test_weird_flag_ask() {
        assert_eq!(classify_command("cargo test --weird").tier, Tier::Ask);
    }

    #[test]
    fn cargo_fmt_reason_not_readonly() {
        let d = classify_command("cargo fmt");
        assert_eq!(d.tier, Tier::AutoAllow);
        // The reason must NOT claim "read-only".
        assert!(
            !d.reason.contains("read-only"),
            "cargo fmt reason wrongly says read-only: {}", d.reason
        );
        assert!(
            d.reason.contains("git-restorable"),
            "cargo fmt reason should mention git-restorable: {}", d.reason
        );
    }

    // ── Fix 2: rm -R / -Rf (capital R) ───────────────────────────────────────

    #[test]
    fn rm_capital_rf_auto_deny() {
        assert_eq!(classify_command("rm -Rf foo").tier, Tier::AutoDeny);
    }

    #[test]
    fn rm_capital_r_only_auto_deny() {
        assert_eq!(classify_command("rm -R foo").tier, Tier::AutoDeny);
    }

    #[test]
    fn rm_mixed_case_rf_auto_deny() {
        assert_eq!(classify_command("rm -rF foo").tier, Tier::AutoDeny);
    }

    // ── Subshell in git arg ───────────────────────────────────────────────────

    #[test]
    fn git_diff_subshell_ask() {
        // `$(cat x)` contains `$(` which the metachar check catches.
        assert_eq!(classify_command("git diff $(cat x)").tier, Tier::Ask);
    }
}
