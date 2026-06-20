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

/// Read-only `git` subcommands.
const GIT_READONLY_SUB: &[&str] = &["diff", "status", "log", "show", "branch"];

/// Read-only `cargo` subcommands.
const CARGO_READONLY_SUB: &[&str] = &["check", "test", "build", "clippy", "fmt", "tree"];

/// Classify a raw shell command string.
///
/// Decision ladder:
///
/// 1. Shell metacharacter or leading env assignment → [`Tier::Ask`]
/// 2. Parse with `shell-words` (on error → [`Tier::Ask`])
/// 3. Hard-deny rules → [`Tier::AutoDeny`]
/// 4. Known read-only allowlist → [`Tier::AutoAllow`]
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

    // `rm` with `-rf`/`-fr`/`-r` or an absolute path arg.
    if prog == "rm" {
        let has_recursive = argv[1..].iter().any(|a| {
            matches!(
                a.as_str(),
                "-rf" | "-fr" | "-r" | "--recursive" | "-rf/" | "-fr/"
            ) || (a.starts_with('-') && a.contains('r') && a.contains('f'))
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

    if prog == "git" && GIT_READONLY_SUB.contains(&sub) {
        return Decision {
            tier: Tier::AutoAllow,
            reason: format!("`git {sub}` is a read-only git operation"),
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
    }

    if prog == "cargo" && CARGO_READONLY_SUB.contains(&sub) {
        return Decision {
            tier: Tier::AutoAllow,
            reason: format!("`cargo {sub}` is a read-only or build-only cargo subcommand"),
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
}
