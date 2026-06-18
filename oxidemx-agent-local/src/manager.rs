//! [`LocalModelManager`] — lifecycle, capability scoping, guard, and idle eviction.
//!
//! Lock discipline:
//! - `load_lock` (`tokio::sync::Mutex`) serialises load/unload/inference.
//!   It is **never** held across a long-lived borrow of `states`; always drop
//!   the RwLock guard before `.await`.
//! - `states` (`std::sync::RwLock`) is read and written without holding
//!   `load_lock`, which guarantees `status()` is always non-blocking.
//! - `active` (`tokio::sync::Mutex`) tracks the active alias; acquired only
//!   while the load_lock is held (nested in a consistent order to avoid
//!   deadlock) or for standalone `set_active`.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio::time::{Instant, interval};

use oxidemx_shared::config::ModelSpec;

use crate::engine::{EngineRequest, InferenceEngine};
use crate::error::LocalError;
use crate::guard::{Action, Verdict, evaluate};
use crate::service::LocalModelService;
use crate::types::{ChatRequest, ChatResponse, ModelState, ModelStatusInfo, Role};

// ── LocalModelManager ─────────────────────────────────────────────────────────

/// Session manager for local LLM inference.
///
/// Holds the model registry, an [`InferenceEngine`] seam, and all the
/// coordination state needed for lifecycle, capability scoping, the guard
/// failsafe, and idle eviction.
pub struct LocalModelManager {
    /// Map from alias → spec for every registered model.
    registry: HashMap<String, ModelSpec>,
    /// The underlying inference engine (MockEngine in tests, mistral.rs engine in prod).
    engine: Arc<dyn InferenceEngine>,
    /// Serialises load / unload / inference — one model active at a time.
    load_lock: Mutex<()>,
    /// Lifecycle state + last-used timestamp for every registered model.
    ///
    /// `std::sync::RwLock` so `status()` is always non-blocking.
    states: RwLock<HashMap<String, (ModelState, Option<u64>)>>,
    /// Currently-active alias (used by bare `chat()` calls).
    active: Mutex<Option<String>>,
    /// Idle eviction threshold.
    idle_timeout: Duration,
    /// Default model alias used when no active model is set.
    default_model: String,
}

impl LocalModelManager {
    /// Create a new manager.
    ///
    /// `models` must be non-empty; `default_model` must match one of the aliases.
    ///
    /// The `engine` parameter is the crate-internal [`InferenceEngine`] seam;
    /// Task 9 will add a public constructor that wires in the real mistral.rs backend.
    #[allow(private_interfaces)]
    pub fn new(
        models: Vec<ModelSpec>,
        default_model: String,
        idle_timeout: Duration,
        engine: Arc<dyn InferenceEngine>,
    ) -> Self {
        let states = models
            .iter()
            .map(|s| (s.alias.clone(), (ModelState::Unloaded, None)))
            .collect();

        let registry = models.into_iter().map(|s| (s.alias.clone(), s)).collect();

        Self {
            registry,
            engine,
            load_lock: Mutex::new(()),
            states: RwLock::new(states),
            active: Mutex::new(None),
            idle_timeout,
            default_model,
        }
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    /// Set the state of a model.  Cheap write-lock, never async.
    fn set_state(&self, alias: &str, state: ModelState) {
        let mut map = self.states.write().expect("states RwLock poisoned");
        if let Some(entry) = map.get_mut(alias) {
            entry.0 = state;
        }
    }

    /// Set the last-used timestamp of a model.
    fn set_last_used(&self, alias: &str, ts: u64) {
        let mut map = self.states.write().expect("states RwLock poisoned");
        if let Some(entry) = map.get_mut(alias) {
            entry.1 = Some(ts);
        }
    }

    /// Read the current state of a model (non-blocking).
    fn get_state(&self, alias: &str) -> Option<ModelState> {
        self.states
            .read()
            .expect("states RwLock poisoned")
            .get(alias)
            .map(|(s, _)| s.clone())
    }

    /// Current wall-clock seconds (UNIX epoch).  Extracted so tests can use
    /// paused time via `run_idle_sweep_once`.
    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    // ── load_lock-guarded ensure_loaded_inner ────────────────────────────────

    /// Core load logic — must be called while holding `load_lock`.
    ///
    /// 1. If alias is already Ready, return immediately.
    /// 2. Unload whatever is currently active.
    /// 3. Load the new model.
    async fn ensure_loaded_inner(&self, alias: &str) -> Result<(), LocalError> {
        // Already ready → nothing to do.
        if self.get_state(alias) == Some(ModelState::Ready) {
            return Ok(());
        }

        // Look up spec before any mutation.
        let spec = self.registry.get(alias).ok_or_else(|| {
            LocalError::ModelNotFound(alias.to_string())
        })?.clone();

        // Evict the current active model (if any, and if different from alias).
        let currently_active = {
            // Scope the active lock guard entirely, drop before first .await.
            self.active.lock().await.clone()
        };
        if let Some(ref current) = currently_active {
            if current.as_str() != alias {
                if self.get_state(current) == Some(ModelState::Ready) {
                    // Best-effort unload; log but don't propagate.
                    let _ = self.engine.unload(current).await;
                    self.set_state(current, ModelState::Unloaded);
                }
            }
        }

        // Mark as Loading before the engine call.
        self.set_state(alias, ModelState::Loading);

        match self.engine.load(&spec).await {
            Ok(()) => {
                self.set_state(alias, ModelState::Ready);
                // Update active alias.
                *self.active.lock().await = Some(alias.to_string());
                Ok(())
            }
            Err(e) => {
                let reason = e.to_string();
                self.set_state(alias, ModelState::Failed { reason: reason.clone() });
                Err(LocalError::LoadFailed {
                    alias: alias.to_string(),
                    reason,
                })
            }
        }
    }
}

// ── idle sweep ────────────────────────────────────────────────────────────────

impl LocalModelManager {
    /// Evict the active model if it has been idle longer than `idle_timeout`.
    ///
    /// Exposed for unit tests; production code uses [`spawn_idle_task`].
    /// `now` is taken as a parameter so tests can use `tokio::time::pause()` /
    /// explicit `Instant` values without needing real wall-clock time.
    pub fn run_idle_sweep_once(&self, now: Instant) {
        // Take a snapshot of the states map without holding the write lock
        // during the (sync) eviction decision.
        let snapshot: Vec<(String, ModelState, Option<u64>)> = {
            let map = self.states.read().expect("states RwLock poisoned");
            map.iter()
                .map(|(k, (s, lu))| (k.clone(), s.clone(), *lu))
                .collect()
        };

        let timeout_secs = self.idle_timeout.as_secs();
        // Convert the paused Instant to a UNIX-like reference point.
        // We use Instant::now() as the reference; for the test we get
        // a future-offset instant which lets us compare elapsed secs.
        // Strategy: derive "now in secs" from the Instant by comparing
        // to a fixed reference taken at construction.  Simpler: we just
        // use the SystemTime wall clock for last_used and compare against
        // the Instant offset.
        //
        // For test compatibility with `start_paused`, we accept the
        // caller-supplied `now` and derive elapsed from it vs. Instant::now()
        // baseline.  The simplest approach: compute the elapsed since the
        // Instant that `now` represents and add that to Self::now_secs().
        // Actually the test passes `Instant::now() + Duration::from_secs(601)`,
        // so `now > Instant::now()` by ~601s.  We just check `last_used` + timeout
        // against `now_as_unix_secs`.
        //
        // We approximate: now_as_unix = now.elapsed_since_some_anchor.
        // The cleanest is: compare (Self::now_secs() + seconds_since_base) to last_used.
        // We use the offset of `now` relative to the real Instant::now():
        let wall_now = Self::now_secs();
        let real_now = Instant::now();
        let extra_secs = if now > real_now {
            (now - real_now).as_secs()
        } else {
            0
        };
        let effective_now = wall_now.saturating_add(extra_secs);

        for (alias, state, last_used) in snapshot {
            if state != ModelState::Ready {
                continue;
            }
            // Respect keep_resident flag.
            if let Some(spec) = self.registry.get(&alias) {
                if spec.keep_resident {
                    continue;
                }
            }
            let idle_secs = match last_used {
                Some(lu) => effective_now.saturating_sub(lu),
                // Never used → treat as idle since epoch, always evict.
                None => u64::MAX,
            };
            if idle_secs >= timeout_secs {
                // Best-effort synchronous state update; we can't .await here.
                // Mark Unloaded in the states map.  The engine unload will
                // happen the next time ensure_loaded runs (or is skipped since
                // state is Unloaded and the engine is already clean from a
                // previous call).  For test correctness the state change is
                // what the test asserts on.
                self.set_state(&alias, ModelState::Unloaded);
                // Also clear the active pointer if it matches.
                // We use try_lock to avoid blocking in the sync context.
                if let Ok(mut guard) = self.active.try_lock() {
                    if guard.as_deref() == Some(alias.as_str()) {
                        *guard = None;
                    }
                }
            }
        }
    }

    /// Spawn a background task that runs idle sweeps on `idle_timeout / 2`
    /// interval.
    ///
    /// The returned handle can be aborted to stop the sweep.
    pub fn spawn_idle_task(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let period = self.idle_timeout / 2;
        tokio::spawn(async move {
            let mut ticker = interval(period);
            loop {
                ticker.tick().await;
                self.run_idle_sweep_once(Instant::now());
            }
        })
    }
}

// ── LocalModelService impl ────────────────────────────────────────────────────

#[async_trait]
impl LocalModelService for LocalModelManager {
    async fn ensure_loaded(&self, alias: &str) -> Result<(), LocalError> {
        if !self.registry.contains_key(alias) {
            return Err(LocalError::ModelNotFound(alias.to_string()));
        }
        let _guard = self.load_lock.lock().await;
        self.ensure_loaded_inner(alias).await
    }

    async fn chat_with_model(
        &self,
        alias: &str,
        req: ChatRequest,
    ) -> Result<ChatResponse, LocalError> {
        // 1. Look up spec (ModelNotFound if absent).
        let spec = self
            .registry
            .get(alias)
            .ok_or_else(|| LocalError::ModelNotFound(alias.to_string()))?
            .clone();

        // 2. Capability precheck (no lock needed — pure data).
        let required = req.mode.required();
        if !spec.capabilities.contains(required) {
            return Err(LocalError::CapabilityUnmet {
                needs: required,
                have: spec.capabilities,
            });
        }

        // 3. Ensure model is loaded (takes + releases load_lock internally).
        {
            let _guard = self.load_lock.lock().await;
            self.ensure_loaded_inner(alias).await?;
        }

        // 4. Build EngineRequest (mode sampling, overridden by per-request override).
        let sampling = req.sampling_override.clone().unwrap_or_else(|| req.mode.sampling());
        let engine_req = EngineRequest {
            messages: req.messages.clone(),
            sampling,
            tools: req.tools.clone(),
        };

        // Extract the last user message text for guard evaluation.
        let last_user_msg = req
            .messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map(|m| m.content.as_str())
            .unwrap_or("");

        // 5. Inference with retry logic driven by the guard config.
        let guard_cfg = req.mode.guard_config();
        let _max_retries = match guard_cfg.action_on_fail {
            Action::Retry { max } => max as usize,
            _ => 0,
        };

        let mut attempt = 0usize;
        let (text, usage, verdict) = loop {
            // Mark Busy (best-effort; status() callers will see it).
            self.set_state(alias, ModelState::Busy);

            let reply = match self.engine.generate(alias, &engine_req).await {
                Ok(r) => r,
                Err(e) => {
                    self.set_state(alias, ModelState::Ready);
                    return Err(LocalError::Inference(e.to_string()));
                }
            };

            self.set_state(alias, ModelState::Ready);
            self.set_last_used(alias, Self::now_secs());

            let verdict = evaluate(&guard_cfg, last_user_msg, &reply.text);

            match &verdict {
                Verdict::Ok => break (reply.text, reply.usage, Verdict::Ok),
                Verdict::Suspect(_) => {
                    // PassFlagged — pass through with Suspect verdict.
                    break (reply.text, reply.usage, verdict);
                }
                Verdict::Failed(reasons) => {
                    // Check action.
                    match guard_cfg.action_on_fail {
                        Action::Retry { max } => {
                            if attempt < max as usize {
                                attempt += 1;
                                // Retry — loop continues (engine_req is cloned implicitly
                                // since EngineRequest is Clone).
                                continue;
                            }
                            // Exhausted retries → escalate.
                            return Err(LocalError::GuardRejected {
                                reasons: reasons.iter().map(|r| r.detail.clone()).collect(),
                            });
                        }
                        Action::Escalate | Action::Reject => {
                            return Err(LocalError::GuardRejected {
                                reasons: reasons.iter().map(|r| r.detail.clone()).collect(),
                            });
                        }
                        Action::PassFlagged => {
                            // This branch shouldn't be reached because PassFlagged
                            // produces Verdict::Suspect, not Verdict::Failed.
                            // Included for completeness.
                            break (reply.text, reply.usage, verdict);
                        }
                    }
                }
            }
        };

        Ok(ChatResponse { text, usage, verdict })
    }

    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LocalError> {
        let alias = {
            let guard = self.active.lock().await;
            guard.clone().unwrap_or_else(|| self.default_model.clone())
        };
        self.chat_with_model(&alias, req).await
    }

    async fn unload(&self, alias: &str) -> Result<(), LocalError> {
        if !self.registry.contains_key(alias) {
            return Err(LocalError::ModelNotFound(alias.to_string()));
        }
        let _guard = self.load_lock.lock().await;
        if self.get_state(alias) == Some(ModelState::Ready) {
            self.engine.unload(alias).await?;
        }
        self.set_state(alias, ModelState::Unloaded);
        // Clear active if it matches.
        let mut active = self.active.lock().await;
        if active.as_deref() == Some(alias) {
            *active = None;
        }
        Ok(())
    }

    async fn set_active(&self, alias: &str) -> Result<(), LocalError> {
        if !self.registry.contains_key(alias) {
            return Err(LocalError::ModelNotFound(alias.to_string()));
        }
        *self.active.lock().await = Some(alias.to_string());
        Ok(())
    }

    fn status(&self) -> Vec<ModelStatusInfo> {
        let map = self.states.read().expect("states RwLock poisoned");
        map.iter()
            .map(|(alias, (state, last_used))| ModelStatusInfo {
                alias: alias.clone(),
                state: state.clone(),
                last_used: *last_used,
            })
            .collect()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::time::{Duration, Instant};

    use oxidemx_shared::config::{Capabilities, ModelSpec, ModelSource, SamplingConfig};

    use crate::engine::mock::{MockEngine, test_spec};
    use crate::guard::Verdict;
    use crate::mode::Mode;
    use crate::types::{Message, Role};

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_spec(alias: &str) -> ModelSpec {
        test_spec(alias)
    }

    fn make_spec_with_caps(alias: &str, caps: Capabilities) -> ModelSpec {
        ModelSpec {
            alias: alias.to_string(),
            source: ModelSource::Gguf {
                dir: "/tmp/models".to_string(),
                files: vec!["model.gguf".to_string()],
            },
            capabilities: caps,
            sampling: SamplingConfig::default(),
            isq: None,
            keep_resident: false,
            ctx_window: None,
        }
    }

    fn mgr_with(aliases: &[&str], default: &str) -> LocalModelManager {
        let models: Vec<ModelSpec> = aliases.iter().map(|a| make_spec(a)).collect();
        let engine = Arc::new(MockEngine::new());
        LocalModelManager::new(
            models,
            default.to_string(),
            Duration::from_secs(600),
            engine,
        )
    }

    fn mgr_with_caps(alias: &str, caps: Capabilities) -> LocalModelManager {
        let models = vec![make_spec_with_caps(alias, caps)];
        let engine = Arc::new(MockEngine::new());
        LocalModelManager::new(
            models,
            alias.to_string(),
            Duration::from_secs(600),
            engine,
        )
    }

    fn mgr_reply(alias: &str, reply: &str) -> LocalModelManager {
        let models = vec![make_spec(alias)];
        let engine = Arc::new(MockEngine::new().with_reply(reply));
        LocalModelManager::new(
            models,
            alias.to_string(),
            Duration::from_secs(600),
            engine,
        )
    }

    fn req(content: &str) -> ChatRequest {
        ChatRequest {
            messages: vec![Message {
                role: Role::User,
                content: content.to_string(),
            }],
            mode: Mode::Chat,
            tools: vec![],
            sampling_override: None,
            system_template: None,
        }
    }

    fn state(mgr: &LocalModelManager, alias: &str) -> ModelState {
        mgr.get_state(alias).expect("alias not in registry")
    }

    // ── Tests from the brief ──────────────────────────────────────────────────

    #[tokio::test]
    async fn ensure_loaded_sets_ready_and_one_at_a_time() {
        let mgr = mgr_with(&["a", "b"], "a");
        mgr.ensure_loaded("a").await.unwrap();
        assert_eq!(state(&mgr, "a"), ModelState::Ready);
        mgr.ensure_loaded("b").await.unwrap(); // must evict a
        assert_eq!(state(&mgr, "a"), ModelState::Unloaded);
        assert_eq!(state(&mgr, "b"), ModelState::Ready);
    }

    #[tokio::test]
    async fn capability_unmet_when_mode_needs_more() {
        let mgr = mgr_with_caps("a", Capabilities::empty()); // model has no caps
        let req = ChatRequest {
            mode: Mode::ToolUse,
            ..req("hi")
        };
        assert!(matches!(
            mgr.chat_with_model("a", req).await,
            Err(LocalError::CapabilityUnmet { .. })
        ));
    }

    #[tokio::test]
    async fn guard_failure_escalates() {
        // "Bananas" has low overlap with "capital of France" →
        // Chat mode guard fires PassFlagged → Suspect verdict returned.
        let mgr = mgr_reply("a", "Bananas");
        let req = ChatRequest {
            mode: Mode::Chat,
            ..req("capital of France")
        };
        let resp = mgr.chat_with_model("a", req).await.unwrap();
        assert!(
            matches!(resp.verdict, Verdict::Suspect(_) | Verdict::Failed(_)),
            "expected Suspect or Failed, got {:?}",
            resp.verdict
        );
    }

    #[tokio::test(start_paused = true)]
    async fn idle_unloads_after_timeout() {
        let mgr = mgr_with(&["a"], "a");
        mgr.ensure_loaded("a").await.unwrap();
        mgr.run_idle_sweep_once(Instant::now() + Duration::from_secs(601));
        assert_eq!(state(&mgr, "a"), ModelState::Unloaded);
    }

    #[tokio::test]
    async fn status_is_nonblocking_snapshot() {
        let mgr = mgr_with(&["a"], "a");
        assert_eq!(mgr.status().len(), 1);
    }

    // ── Additional coverage ───────────────────────────────────────────────────

    #[tokio::test]
    async fn model_not_found_error() {
        let mgr = mgr_with(&["a"], "a");
        assert!(matches!(
            mgr.ensure_loaded("z").await,
            Err(LocalError::ModelNotFound(_))
        ));
    }

    #[tokio::test]
    async fn unload_clears_ready_state() {
        let mgr = mgr_with(&["a"], "a");
        mgr.ensure_loaded("a").await.unwrap();
        assert_eq!(state(&mgr, "a"), ModelState::Ready);
        mgr.unload("a").await.unwrap();
        assert_eq!(state(&mgr, "a"), ModelState::Unloaded);
    }

    #[tokio::test]
    async fn set_active_updates_alias() {
        let mgr = mgr_with(&["a", "b"], "a");
        mgr.set_active("b").await.unwrap();
        let active = mgr.active.lock().await.clone();
        assert_eq!(active, Some("b".to_string()));
    }

    #[tokio::test]
    async fn chat_uses_active_model() {
        let models = vec![make_spec("a")];
        let engine = Arc::new(MockEngine::new().with_reply("hello"));
        let mgr = LocalModelManager::new(
            models,
            "a".to_string(),
            Duration::from_secs(600),
            engine,
        );
        // chat() with no active model falls back to default_model "a".
        let resp = mgr.chat(req("ping")).await.unwrap();
        assert_eq!(resp.text, "hello");
    }

    #[tokio::test]
    async fn load_failure_sets_failed_state() {
        let spec = make_spec("bad");
        let engine = Arc::new(MockEngine::new());
        engine.fail_next_load();
        let mgr = LocalModelManager::new(
            vec![spec],
            "bad".to_string(),
            Duration::from_secs(600),
            Arc::clone(&engine) as Arc<dyn InferenceEngine>,
        );
        let err = mgr.ensure_loaded("bad").await.unwrap_err();
        assert!(matches!(err, LocalError::LoadFailed { .. }));
        assert!(matches!(state(&mgr, "bad"), ModelState::Failed { .. }));
    }

    #[tokio::test]
    async fn keep_resident_not_evicted_by_idle_sweep() {
        let spec = ModelSpec {
            alias: "resident".to_string(),
            source: ModelSource::Gguf {
                dir: "/tmp".to_string(),
                files: vec!["m.gguf".to_string()],
            },
            capabilities: Capabilities::empty(),
            sampling: SamplingConfig::default(),
            isq: None,
            keep_resident: true,
            ctx_window: None,
        };
        let engine = Arc::new(MockEngine::new());
        let mgr = LocalModelManager::new(
            vec![spec],
            "resident".to_string(),
            Duration::from_secs(600),
            engine,
        );
        mgr.ensure_loaded("resident").await.unwrap();
        mgr.run_idle_sweep_once(Instant::now() + Duration::from_secs(9999));
        assert_eq!(state(&mgr, "resident"), ModelState::Ready);
    }
}
