//! Schema-gated plan generation with a retry / escalate recovery loop.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use oxidemx_ledger::{Step, StepGraph};

use crate::error::PlannerError;
use crate::model::{PlanRequest, PlannerModel};

// ──────────────────────────────────────────────────────────────────────────────
// Wire types (what the model emits)
// ──────────────────────────────────────────────────────────────────────────────

/// Wire shape of a single step as the model emits it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PlanStep {
    /// Unique identifier for this step within the plan.
    pub id: String,
    /// Short human-readable title.
    pub title: String,
    /// IDs of steps that must complete before this one can start.
    #[serde(default)]
    pub needs: Vec<String>,
}

/// Top-level wire shape the model must emit.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PlanOutput {
    /// Ordered list of steps in the plan.
    pub steps: Vec<PlanStep>,
}

// ──────────────────────────────────────────────────────────────────────────────
// Planner
// ──────────────────────────────────────────────────────────────────────────────

/// Turns a free-form goal into a validated [`StepGraph`] via a language model.
///
/// The planner:
/// 1. Derives a JSON Schema for [`PlanOutput`] via `schemars` and forwards it
///    to the model as a generate-time constraint.
/// 2. Validates the raw reply against that schema with `jsonschema` at
///    *validate time* — independent of whatever the model did.
/// 3. On failure, retries up to `max_local_retries` times, feeding the
///    validator / parse / graph error back as a corrective system note.
/// 4. If local retries are exhausted, makes one escalated call
///    (`escalate = true`).
/// 5. If that also fails, returns [`PlannerError::Unresolved`] with all
///    accumulated failure reasons.
pub struct Planner<M: PlannerModel> {
    model: M,
    max_local_retries: u32,
}

impl<M: PlannerModel> Planner<M> {
    /// Create a new planner backed by `model`, allowing up to
    /// `max_local_retries` local retry attempts before escalating.
    pub fn new(model: M, max_local_retries: u32) -> Self {
        Self {
            model,
            max_local_retries,
        }
    }

    /// Generate and validate a [`StepGraph`] for the given `goal`.
    pub async fn plan(&self, goal: &str) -> Result<StepGraph, PlannerError> {
        // 1. Derive the JSON Schema for PlanOutput once.
        let schema_root = schemars::schema_for!(PlanOutput);
        let json_schema: Value = serde_json::to_value(&schema_root)
            .map_err(|e| PlannerError::SchemaInvalid(e.to_string()))?;

        // Build a jsonschema validator from the derived schema.
        let validator = jsonschema::validator_for(&json_schema)
            .map_err(|e| PlannerError::SchemaInvalid(e.to_string()))?;

        let mut reasons: Vec<String> = Vec::new();
        let mut system_note = String::new();

        // 2. Local retry loop.
        for attempt in 0..=self.max_local_retries {
            let escalate = false;
            match self
                .attempt_once(
                    goal,
                    &json_schema,
                    &validator,
                    &system_note,
                    escalate,
                )
                .await
            {
                Ok(graph) => return Ok(graph),
                Err(reason) => {
                    reasons.push(reason.clone());
                    system_note = reason;
                    // After the last local attempt, break to the escalation path.
                    if attempt == self.max_local_retries {
                        break;
                    }
                }
            }
        }

        // 3. Escalation call.
        match self
            .attempt_once(goal, &json_schema, &validator, &system_note, true)
            .await
        {
            Ok(graph) => return Ok(graph),
            Err(reason) => {
                reasons.push(reason);
            }
        }

        Err(PlannerError::Unresolved { reasons })
    }

    /// Perform a single model call + validate-time gate + graph validation.
    ///
    /// Returns `Ok(StepGraph)` on success, `Err(reason_string)` on any failure.
    async fn attempt_once(
        &self,
        goal: &str,
        json_schema: &Value,
        validator: &jsonschema::Validator,
        system_note: &str,
        escalate: bool,
    ) -> Result<StepGraph, String> {
        let req = PlanRequest {
            system: system_note.to_string(),
            goal: goal.to_string(),
            json_schema: json_schema.clone(),
            escalate,
        };

        // Call the model.
        let raw = self
            .model
            .complete(req)
            .await
            .map_err(|e| e.to_string())?;

        // Parse as JSON.
        let instance: Value = serde_json::from_str(&raw)
            .map_err(|e| format!("JSON parse error: {e}"))?;

        // Validate-time: check against the derived schema.
        let errors: Vec<String> = validator
            .iter_errors(&instance)
            .map(|e| e.to_string())
            .collect();
        if !errors.is_empty() {
            return Err(format!("Schema validation failed: {}", errors.join("; ")));
        }

        // Deserialize to PlanOutput.
        let plan_output: PlanOutput = serde_json::from_value(instance)
            .map_err(|e| format!("Deserialization error: {e}"))?;

        // Map wire types → ledger types and validate the DAG.
        let steps: Vec<Step> = plan_output
            .steps
            .into_iter()
            .map(|ps| {
                let mut s = Step::new(&ps.id, &ps.title);
                s.needs = ps.needs;
                s
            })
            .collect();

        let graph = StepGraph::new(steps);
        graph
            .validate()
            .map_err(|e| format!("Graph validation error: {e}"))?;

        Ok(graph)
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::PlannerError;
    use crate::model::MockPlannerModel;

    #[tokio::test]
    async fn plan_returns_valid_stepgraph() {
        let model = MockPlannerModel::scripted(vec![
            r#"{"steps":[{"id":"a","title":"first","needs":[]},
                         {"id":"b","title":"second","needs":["a"]}]}"#
                .into(),
        ]);
        let p = Planner::new(model, 2);
        let g = p.plan("do the thing").await.unwrap();
        assert_eq!(g.steps().len(), 2);
        assert!(g.validate().is_ok());
    }

    #[tokio::test]
    async fn plan_recovers_from_invalid_then_valid() {
        let model = MockPlannerModel::scripted(vec![
            "not json at all".into(), // attempt 1: schema-invalid
            r#"{"steps":[{"id":"a","title":"x","needs":[]}]}"#.into(), // attempt 2: valid
        ]);
        let p = Planner::new(model, 2);
        assert!(p.plan("g").await.is_ok()); // recovered on retry
    }

    #[tokio::test]
    async fn plan_rejects_cyclic_plan() {
        let model = MockPlannerModel::scripted(vec![
            r#"{"steps":[{"id":"a","title":"A","needs":["b"]},
                         {"id":"b","title":"B","needs":["a"]}]}"#
                .into(), // valid JSON, cyclic graph
            r#"{"steps":[{"id":"a","title":"A","needs":["b"]},
                         {"id":"b","title":"B","needs":["a"]}]}"#
                .into(), // escalate also cyclic
        ]);
        let p = Planner::new(model, 1);
        assert!(matches!(
            p.plan("g").await,
            Err(PlannerError::Unresolved { .. })
        )); // graph gate caught it
    }
}
