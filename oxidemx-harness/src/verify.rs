//! Verification command runner and Verifier trait.

use async_trait::async_trait;
use oxidemx_ledger::CompletionPromise;
use std::path::Path;
use thiserror::Error;

/// Result of a command execution.
#[derive(Clone, Debug)]
pub struct CommandResult {
    /// Whether the command succeeded (exit code 0).
    pub ok: bool,
    /// Standard output from the command.
    pub stdout: String,
    /// Standard error from the command.
    pub stderr: String,
}

/// Error type for verification failures.
#[derive(Clone, Debug, Error)]
#[error("verification failed")]
pub struct VerifyFailure {
    /// Output from the failed verification (stderr + stdout, truncated).
    pub output: String,
}

/// Trait for running commands. Allows mocking in tests.
#[async_trait]
pub trait CommandRunner: Send + Sync {
    /// Run a command and return the result.
    async fn run(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
    ) -> CommandResult;
}

/// Verifies step completion by running a command.
pub struct Verifier<R: CommandRunner> {
    runner: R,
}

impl<R: CommandRunner> Verifier<R> {
    /// Create a new Verifier with the given command runner.
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    /// Verify a step by running the verification command.
    ///
    /// On success, returns a `CompletionPromise` with a deterministic token.
    /// On failure, returns a `VerifyFailure` with truncated stderr + stdout.
    pub async fn verify(
        &self,
        step_id: &str,
        program: &str,
        args: &[String],
        cwd: &Path,
        now: u64,
    ) -> Result<CompletionPromise, VerifyFailure> {
        let result = self.runner.run(program, args, cwd).await;

        if result.ok {
            // Hash the ok + stdout to generate a deterministic token.
            let token = self.generate_token(true, &result.stdout);
            Ok(CompletionPromise {
                step_id: step_id.to_string(),
                verifier: program.to_string(),
                token,
                ts: now,
            })
        } else {
            // Combine stderr and stdout for the critique output, truncated to a reasonable size.
            let mut output = result.stderr.clone();
            if !output.is_empty() && !result.stdout.is_empty() {
                output.push('\n');
            }
            output.push_str(&result.stdout);

            // Truncate to ~4KB to avoid bloating the critique loop.
            const MAX_OUTPUT: usize = 4096;
            if output.len() > MAX_OUTPUT {
                output.truncate(MAX_OUTPUT);
                output.push_str("\n... (truncated)");
            }

            Err(VerifyFailure { output })
        }
    }

    /// Generate a short deterministic token from the success status and stdout.
    fn generate_token(&self, ok: bool, stdout: &str) -> String {
        // Use FNV-1a hash (same as oxidemx-ledger) for consistency.
        let input = format!("{}{}", ok, stdout);
        let hash = fnv1a64(input.as_bytes());
        format!("{:016x}", hash)
    }
}

/// Computes FNV-1a 64-bit hash (same as oxidemx-ledger for consistency).
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x00000100000001b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock command runner for testing.
    struct MockRunner {
        ok: bool,
        stdout: String,
        stderr: String,
    }

    impl MockRunner {
        /// Create a mock that returns success with the given stdout.
        fn ok(stdout: &str) -> Self {
            Self {
                ok: true,
                stdout: stdout.to_string(),
                stderr: String::new(),
            }
        }

        /// Create a mock that returns failure with the given stderr.
        fn fail(stderr: &str) -> Self {
            Self {
                ok: false,
                stdout: String::new(),
                stderr: stderr.to_string(),
            }
        }
    }

    #[async_trait]
    impl CommandRunner for MockRunner {
        async fn run(
            &self,
            _program: &str,
            _args: &[String],
            _cwd: &Path,
        ) -> CommandResult {
            CommandResult {
                ok: self.ok,
                stdout: self.stdout.clone(),
                stderr: self.stderr.clone(),
            }
        }
    }

    #[tokio::test]
    async fn verify_pass_yields_promise() {
        let v = Verifier::new(MockRunner::ok("Finished test"));
        let p = v
            .verify("a", "cargo", &["test".into()], Path::new("."), 7)
            .await
            .unwrap();
        assert_eq!(p.step_id, "a");
        assert!(!p.token.is_empty());
    }

    #[tokio::test]
    async fn verify_fail_yields_critique() {
        let v = Verifier::new(MockRunner::fail("error[E0308]: mismatched types"));
        let f = v
            .verify("a", "cargo", &["check".into()], Path::new("."), 7)
            .await
            .unwrap_err();
        assert!(f.output.contains("E0308"));
    }
}
