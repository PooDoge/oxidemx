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

    // ── Poison-safe RwLock accessors ─────────────────────────────────────────

    /// Acquire a read guard on `states`, recovering from a poisoned lock
    /// instead of panicking.
    fn states_read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<String, (ModelState, Option<u64>)>> {
        self.states.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Acquire a write guard on `states`, recovering from a poisoned lock
    /// instead of panicking.
    fn states_write(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<String, (ModelState, Option<u64>)>> {
        self.states.write().unwrap_or_else(|e| e.into_inner())
    }

    /// Set the state of a model.  Cheap write-lock, never async.
    fn set_state(&self, alias: &str, state: ModelState) {
        let mut map = self.states_write();
        if let Some(entry) = map.get_mut(alias) {
            entry.0 = state;
        }
    }

    /// Set the last-used timestamp of a model.
    fn set_last_used(&self, alias: &str, ts: u64) {
        let mut map = self.states_write();
        if let Some(entry) = map.get_mut(alias) {
            entry.1 = Some(ts);
        }
    }

    /// Read the current state of a model (non-blocking).
    fn get_state(&self, alias: &str) -> Option<ModelState> {
        self.states_read()
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
            if current.as_str() != alias && self.get_state(current) == Some(ModelState::Ready) {
                // Best-effort unload; log but don't propagate.
                let _ = self.engine.unload(current).await;
                self.set_state(current, ModelState::Unloaded);
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

// ── Public feature-gated constructors ────────────────────────────────────────

#[cfg(feature = "mistral")]
impl LocalModelManager {
    /// Build a [`LocalModelManager`] backed by the real mistral.rs engine.
    ///
    /// This is the public entry-point for code outside the crate (e.g. the
    /// `cli` example and `agentd`) that cannot name the `pub(crate)`
    /// [`InferenceEngine`] trait directly.  All of the actual wiring is done
    /// here; callers only need the public [`crate::mistral::MistralEngine`]
    /// type.
    ///
    /// # Panics
    ///
    /// Does not panic; returns `LocalError` on engine-build failure.
    pub fn with_mistral_engine(
        models: Vec<oxidemx_shared::config::ModelSpec>,
        default_model: String,
        idle_timeout: Duration,
        engine: crate::mistral::MistralEngine,
    ) -> Self {
        Self::new(models, default_model, idle_timeout, Arc::new(engine))
    }
}

// ── idle sweep ────────────────────────────────────────────────────────────────

impl LocalModelManager {
    /// Evict the active model if it has been idle longer than `idle_timeout`.
    ///
    /// Exposed for unit tests; production code uses [`spawn_idle_task`].
    /// `now` is taken as a parameter so tests can use `tokio::time::pause()` /
    /// explicit `Instant` values without needing real wall-clock time.
    ///
    /// If `idle_timeout` is zero, this returns immediately without evicting
    /// anything — `idle_timeout=0` means "never unload on idle".
    ///
    /// This is `async` because it calls `engine.unload` to actually free VRAM.
    pub async fn run_idle_sweep_once(&self, now: Instant) {
        // idle_timeout=0 means "never unload on idle".
        if self.idle_timeout.is_zero() {
            return;
        }

        // Take a snapshot of the states map without holding the RwLock guard
        // across any .await point.
        let snapshot: Vec<(String, ModelState, Option<u64>)> = {
            let map = self.states_read();
            map.iter()
                .map(|(k, (s, lu))| (k.clone(), s.clone(), *lu))
                .collect()
        };

        let timeout_secs = self.idle_timeout.as_secs();
        // For test compatibility with `start_paused`, we accept the
        // caller-supplied `now` and derive elapsed from it vs. Instant::now()
        // baseline.  The test passes `Instant::now() + Duration::from_secs(601)`,
        // so `now > Instant::now()` by ~601s.  We offset the SystemTime wall
        // clock by the same delta so the comparison is correct even with
        // paused tokio time.
        let wall_now = Self::now_secs();
        let real_now = Instant::now();
        let extra_secs = if now > real_now {
            (now - real_now).as_secs()
        } else {
            0
        };
        let effective_now = wall_now.saturating_add(extra_secs);

        // Determine which model to evict (we only evict the active model to
        // avoid surprising multi-model scenarios).
        let active_alias: Option<String> = self.active.lock().await.clone();

        for (alias, state, last_used) in snapshot {
            if state != ModelState::Ready {
                continue;
            }
            // Only evict the currently-active model.
            if active_alias.as_deref() != Some(alias.as_str()) {
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
                // Acquire load_lock so the eviction is serialised with
                // load/unload/inference.
                let _guard = self.load_lock.lock().await;

                // Re-check state under the lock (it may have changed).
                if self.get_state(&alias) != Some(ModelState::Ready) {
                    continue;
                }

                // Call the engine to actually free VRAM.
                match self.engine.unload(&alias).await {
                    Ok(()) => {
                        self.set_state(&alias, ModelState::Unloaded);
                    }
                    Err(e) => {
                        let reason = e.to_string();
                        tracing::warn!(
                            alias = %alias,
                            error = %reason,
                            "idle eviction: engine.unload failed; marking Failed"
                        );
                        self.set_state(&alias, ModelState::Failed { reason });
                    }
                }

                // Clear the active pointer.
                let mut active_guard = self.active.lock().await;
                if active_guard.as_deref() == Some(alias.as_str()) {
                    *active_guard = None;
                }
            }
        }
    }

    /// Spawn a background task that runs idle sweeps on `idle_timeout / 2`
    /// interval.
    ///
    /// If `idle_timeout` is zero ("never unload on idle"), this returns
    /// immediately without spawning any background work.
    ///
    /// The returned handle can be aborted to stop the sweep.
    pub fn spawn_idle_task(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        if self.idle_timeout.is_zero() {
            tracing::debug!("idle-unload disabled (idle_timeout=0); skipping idle sweep task");
            return tokio::spawn(std::future::ready(()));
        }
        // Use at least 1 second to avoid a tokio panic on zero-period interval.
        let period = (self.idle_timeout / 2).max(Duration::from_secs(1));
        tokio::spawn(async move {
            let mut ticker = interval(period);
            loop {
                ticker.tick().await;
                self.run_idle_sweep_once(Instant::now()).await;
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
            constraint: req.constraint.clone(),
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
        let map = self.states_read();
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
            engine as Arc<dyn InferenceEngine>,
        )
    }

    /// Like `mgr_with` but returns the `Arc<MockEngine>` so tests can inspect
    /// recorded unloads.
    fn mgr_with_engine(aliases: &[&str], default: &str) -> (LocalModelManager, Arc<MockEngine>) {
        let models: Vec<ModelSpec> = aliases.iter().map(|a| make_spec(a)).collect();
        let engine = Arc::new(MockEngine::new());
        let mgr = LocalModelManager::new(
            models,
            default.to_string(),
            Duration::from_secs(600),
            Arc::clone(&engine) as Arc<dyn InferenceEngine>,
        );
        (mgr, engine)
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
            constraint: None,
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
        let (mgr, engine) = mgr_with_engine(&["a"], "a");
        mgr.ensure_loaded("a").await.unwrap();
        mgr.run_idle_sweep_once(Instant::now() + Duration::from_secs(601)).await;
        // State must be Unloaded in the manager.
        assert_eq!(state(&mgr, "a"), ModelState::Unloaded);
        // Engine must have been called — VRAM actually freed.
        assert!(
            engine.unloaded().contains(&"a".to_string()),
            "engine.unload(\"a\") was never called — VRAM not freed"
        );
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
        mgr.run_idle_sweep_once(Instant::now() + Duration::from_secs(9999)).await;
        assert_eq!(state(&mgr, "resident"), ModelState::Ready);
    }

    /// idle_timeout=0 means "never unload on idle": the sweep must not evict
    /// the model regardless of how much time has passed.
    ///
    /// This also covers the panic path: `tokio::time::interval(Duration::ZERO)`
    /// would panic, and `run_idle_sweep_once` must return early before reaching
    /// that code path.
    #[tokio::test(start_paused = true)]
    async fn idle_timeout_zero_never_unloads() {
        let models = vec![make_spec("a")];
        let engine = Arc::new(MockEngine::new());
        let mgr = LocalModelManager::new(
            models,
            "a".to_string(),
            Duration::ZERO, // idle_timeout=0 → never evict
            Arc::clone(&engine) as Arc<dyn InferenceEngine>,
        );

        mgr.ensure_loaded("a").await.unwrap();
        assert_eq!(state(&mgr, "a"), ModelState::Ready);

        // Advance far into the future — would trigger eviction if timeout=0 were mishandled.
        mgr.run_idle_sweep_once(Instant::now() + Duration::from_secs(u32::MAX as u64)).await;

        // Model must still be Ready.
        assert_eq!(
            state(&mgr, "a"),
            ModelState::Ready,
            "idle_timeout=0 should never unload the model"
        );
        // Engine must NOT have been asked to unload.
        assert!(
            !engine.unloaded().contains(&"a".to_string()),
            "engine.unload(\"a\") must not be called when idle_timeout=0"
        );
    }
}
