//! Real command runner using tokio::process for actual command execution.

use crate::verify::{CommandResult, CommandRunner};
use async_trait::async_trait;
use std::path::Path;

/// Real command runner that executes commands via tokio::process.
///
/// This implementation never panics on missing programs; spawn failures
/// are converted to failure CommandResults.
#[cfg(feature = "process")]
#[derive(Clone, Copy, Debug)]
pub struct RealCommandRunner;

#[cfg(feature = "process")]
#[async_trait]
impl CommandRunner for RealCommandRunner {
    async fn run(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
    ) -> CommandResult {
        match tokio::process::Command::new(program)
            .args(args)
            .current_dir(cwd)
            .output()
            .await
        {
            Ok(out) => CommandResult {
                ok: out.status.success(),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            },
            Err(e) => CommandResult {
                ok: false,
                stdout: String::new(),
                stderr: format!("spawn failed: {e}"),
            },
        }
    }
}

#[cfg(all(test, feature = "process"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn true_succeeds_false_fails() {
        let r = RealCommandRunner;
        let cwd = std::env::temp_dir();
        assert!(r.run("true", &[], &cwd).await.ok);
        assert!(!r.run("false", &[], &cwd).await.ok);
    }

    #[tokio::test]
    async fn echo_captures_stdout() {
        let r = RealCommandRunner;
        let out = r
            .run("echo", &["hello".into()], &std::env::temp_dir())
            .await;
        assert!(out.ok && out.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn missing_program_is_failure_not_panic() {
        let r = RealCommandRunner;
        let out = r
            .run("definitely-not-a-real-program-xyz", &[], &std::env::temp_dir())
            .await;
        assert!(!out.ok && out.stderr.contains("spawn failed"));
    }
}
