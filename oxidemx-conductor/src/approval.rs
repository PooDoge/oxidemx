//! Approval policy + gate (spec §7.2).
//!
//! Two layers, kept distinct on purpose (the lesson from P1b/the
//! `building-llm-agents-in-rust` skill):
//!
//! 1. **Routine allow/deny of `execute_command`** lives INSIDE the
//!    tool — the bridged `oxidemx_agent::tools::ExecuteCommand` checks
//!    the process allowlist and returns a *structured `Ok` denial* so
//!    the model explains/recovers instead of the turn aborting. The
//!    supervisor installs the effective allowlist before any step runs.
//!
//! 2. **Halting policy** (pause / cancel / an explicit human "no")
//!    uses the executor's `on_tool_call → HookOutcome::Abort` hook.
//!    That's where the `ApprovalGate` plugs in.
//!
//! The gate here is the policy + asker + audit layer the route/spawn
//! paths and agentd (P4, real D-Bus asker) consume. Headless with no
//! asker, an off-allowlist call under an asking policy is denied.

use std::sync::Arc;

use serde_json::Value;
use tokio::sync::Mutex;

use oxidemx_agent::allowlist::is_allowlisted;

/// Per-agent approval policy (flow/step/roster `approval` field).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApprovalPolicy {
    /// Every tool call needs explicit approval (no allowlist fast path).
    Always,
    /// Allowlisted commands run; anything else asks (or denies headless).
    #[default]
    Allowlist,
    /// Trusted flow — every call runs without asking.
    Autonomous,
}

impl ApprovalPolicy {
    /// Parse the flow/step `approval` string; unknown ⇒ `Allowlist`.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "always" => Self::Always,
            "autonomous" => Self::Autonomous,
            _ => Self::Allowlist,
        }
    }
}

/// The gate's decision for a single tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny(String),
}

/// How the gate asks a human when policy requires it. agentd (P4)
/// implements this over the D-Bus approval round-trip; the CLI leaves
/// it `None` (off-allowlist under an asking policy ⇒ deny).
#[async_trait::async_trait]
pub trait Asker: Send + Sync {
    async fn ask(&self, step: &str, tool: &str, args: &Value) -> Verdict;
}

/// One recorded gate decision, for the run's audit log / event console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub step: String,
    pub tool: String,
    pub verdict: Verdict,
}

/// The shared approval gate injected into every step agent.
#[derive(Clone)]
pub struct ApprovalGate {
    policy: ApprovalPolicy,
    allowlist: Arc<Vec<String>>,
    asker: Option<Arc<dyn Asker>>,
    audit: Arc<Mutex<Vec<AuditEntry>>>,
}

impl ApprovalGate {
    pub fn new(policy: ApprovalPolicy, allowlist: Vec<String>, asker: Option<Arc<dyn Asker>>) -> Self {
        Self {
            policy,
            allowlist: Arc::new(allowlist),
            asker,
            audit: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Decide a tool call. `execute_command`'s `command` arg is matched
    /// against the allowlist for the fast path; other tools are
    /// allowed under `Autonomous`/`Allowlist` and ask under `Always`.
    pub async fn check(&self, step: &str, tool: &str, args: &Value) -> Verdict {
        let verdict = self.decide(step, tool, args).await;
        self.audit.lock().await.push(AuditEntry {
            step: step.to_string(),
            tool: tool.to_string(),
            verdict: verdict.clone(),
        });
        verdict
    }

    async fn decide(&self, step: &str, tool: &str, args: &Value) -> Verdict {
        if self.policy == ApprovalPolicy::Autonomous {
            return Verdict::Allow;
        }
        // Allowlist fast path for shell commands.
        if self.policy == ApprovalPolicy::Allowlist && tool == "execute_command" {
            if let Some(cmd) = args.get("command").and_then(Value::as_str) {
                if is_allowlisted(cmd, &self.allowlist) {
                    return Verdict::Allow;
                }
            }
        }
        // Otherwise ask, or deny when there is no human in the loop.
        match &self.asker {
            Some(a) => a.ask(step, tool, args).await,
            None => Verdict::Deny(format!(
                "`{tool}` requires approval under the `{:?}` policy and no approver is attached (headless run)",
                self.policy
            )),
        }
    }

    /// Snapshot the audit log (denials feed the event console).
    pub async fn audit_log(&self) -> Vec<AuditEntry> {
        self.audit.lock().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn policy_parses_case_insensitively_with_fallback() {
        assert_eq!(ApprovalPolicy::parse("Always"), ApprovalPolicy::Always);
        assert_eq!(ApprovalPolicy::parse("AUTONOMOUS"), ApprovalPolicy::Autonomous);
        assert_eq!(ApprovalPolicy::parse("nonsense"), ApprovalPolicy::Allowlist);
    }

    #[tokio::test]
    async fn autonomous_allows_everything() {
        let gate = ApprovalGate::new(ApprovalPolicy::Autonomous, vec![], None);
        assert_eq!(
            gate.check("s", "execute_command", &json!({"command": "rm -rf /"})).await,
            Verdict::Allow
        );
    }

    #[tokio::test]
    async fn allowlist_allows_listed_denies_unlisted_headless() {
        let gate = ApprovalGate::new(ApprovalPolicy::Allowlist, vec!["echo".into()], None);
        assert_eq!(
            gate.check("s", "execute_command", &json!({"command": "echo hi"})).await,
            Verdict::Allow
        );
        assert!(matches!(
            gate.check("s", "execute_command", &json!({"command": "curl evil"})).await,
            Verdict::Deny(_)
        ));
    }

    #[tokio::test]
    async fn asker_is_consulted_when_off_allowlist() {
        struct YesAsker;
        #[async_trait::async_trait]
        impl Asker for YesAsker {
            async fn ask(&self, _: &str, _: &str, _: &Value) -> Verdict {
                Verdict::Allow
            }
        }
        let gate = ApprovalGate::new(
            ApprovalPolicy::Allowlist,
            vec![],
            Some(Arc::new(YesAsker)),
        );
        assert_eq!(
            gate.check("s", "execute_command", &json!({"command": "anything"})).await,
            Verdict::Allow
        );
        assert_eq!(gate.audit_log().await.len(), 1);
    }
}
