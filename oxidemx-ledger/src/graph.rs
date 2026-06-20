//! Directed acyclic graph (DAG) representation of task steps.
//!
//! Provides validation for step graphs, checking for cycles, duplicate IDs,
//! and missing dependencies (orphan nodes).

use crate::model::{Step, TaskManifest, TaskId};
use std::collections::{HashMap, HashSet, VecDeque};
use thiserror::Error;

/// Errors that can occur during graph validation.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum GraphError {
    /// A step ID appears more than once in the graph.
    #[error("Duplicate step ID: {0}")]
    DuplicateId(String),

    /// A step depends on a non-existent step.
    #[error("Missing dependency: step '{step}' needs '{needs}' which does not exist")]
    MissingDep {
        /// The step that has the missing dependency.
        step: String,
        /// The ID of the missing dependency.
        needs: String,
    },

    /// The graph contains a cycle.
    #[error("Cycle detected in graph; steps involved: {}", .0.join(", "))]
    Cycle(Vec<String>),
}

/// A directed acyclic graph (DAG) of steps with validation.
///
/// Represents a task's workflow as a set of steps with dependencies,
/// and provides validation to ensure the graph is acyclic and well-formed.
#[derive(Clone, Debug)]
pub struct StepGraph {
    steps: Vec<Step>,
}

impl StepGraph {
    /// Create a new step graph from a vector of steps.
    ///
    /// The graph is not validated until `validate()` is called.
    pub fn new(steps: Vec<Step>) -> Self {
        Self { steps }
    }

    /// Return a slice of all steps in the graph.
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// Convert this graph into a TaskManifest with the given task ID and goal.
    pub fn into_manifest(self, task_id: TaskId, goal: String) -> TaskManifest {
        let mut manifest = TaskManifest::new(task_id, goal);
        manifest.steps = self.steps;
        manifest
    }

    /// Validate the graph for cycles, duplicate IDs, and missing dependencies.
    ///
    /// Returns an error if:
    /// - Any step ID appears more than once (DuplicateId)
    /// - Any step depends on a non-existent step (MissingDep)
    /// - The graph contains a cycle (Cycle)
    ///
    /// Otherwise, returns Ok(()).
    pub fn validate(&self) -> Result<(), GraphError> {
        // Check for duplicate step IDs
        let mut seen_ids = HashSet::new();
        for step in &self.steps {
            if !seen_ids.insert(&step.id) {
                return Err(GraphError::DuplicateId(step.id.clone()));
            }
        }

        // Check that all dependencies exist
        let step_ids: HashSet<&str> = self.steps.iter().map(|s| s.id.as_str()).collect();
        for step in &self.steps {
            for dep_id in &step.needs {
                if !step_ids.contains(dep_id.as_str()) {
                    return Err(GraphError::MissingDep {
                        step: step.id.clone(),
                        needs: dep_id.clone(),
                    });
                }
            }
        }

        // Check for cycles using Kahn's algorithm
        self.check_acyclic()?;

        Ok(())
    }

    /// Non-fatal advisories about the graph — currently, step ids that are
    /// unreachable from any root (never dequeued during the topological pass).
    /// Unreachable nodes are inert, not an error.
    pub fn warnings(&self) -> Vec<String> {
        // Build in-degree map and adjacency list using Kahn's algorithm,
        // identical to check_acyclic. Collect ids that are never dequeued.
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();

        // Initialize all steps with 0 in-degree and empty adjacency list
        for step in &self.steps {
            in_degree.insert(&step.id, 0);
            adjacency.insert(&step.id, Vec::new());
        }

        // Build edges: if B needs A, add edge A → B
        for step in &self.steps {
            for dep_id in &step.needs {
                // Increment B's in-degree (one more prerequisite)
                if let Some(d) = in_degree.get_mut(step.id.as_str()) {
                    *d += 1;
                }
                // Add B to A's adjacency list
                if let Some(adj) = adjacency.get_mut(dep_id.as_str()) {
                    adj.push(&step.id);
                }
            }
        }

        // Find all nodes with in-degree 0 and add to queue
        let mut queue: VecDeque<&str> = VecDeque::new();
        for (id, &degree) in &in_degree {
            if degree == 0 {
                queue.push_back(id);
            }
        }

        // Process queue, removing zero-in-degree nodes
        let mut processed_ids = HashSet::new();
        while let Some(node) = queue.pop_front() {
            processed_ids.insert(node.to_string());

            // For each node that depends on this one, decrement its in-degree
            if let Some(dependents) = adjacency.get(node) {
                for &dependent in dependents {
                    if let Some(degree) = in_degree.get_mut(dependent) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(dependent);
                        }
                    }
                }
            }
        }

        // Unreachable nodes are those never processed (never dequeued)
        let mut unreachable: Vec<String> = self
            .steps
            .iter()
            .map(|s| &s.id)
            .filter(|id| !processed_ids.contains(id.as_str()))
            .cloned()
            .collect();
        unreachable.sort();
        unreachable
    }

    /// Check if the graph is acyclic using Kahn's topological sort algorithm.
    fn check_acyclic(&self) -> Result<(), GraphError> {
        // Build in-degree map and adjacency list
        // Edge direction: if step B needs step A, then A → B (A must be done before B)
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();

        // Initialize all steps with 0 in-degree and empty adjacency list
        for step in &self.steps {
            in_degree.insert(&step.id, 0);
            adjacency.insert(&step.id, Vec::new());
        }

        // Build edges: if B needs A, add edge A → B
        for step in &self.steps {
            for dep_id in &step.needs {
                // Increment B's in-degree (one more prerequisite)
                if let Some(d) = in_degree.get_mut(step.id.as_str()) {
                    *d += 1;
                }
                // Add B to A's adjacency list
                if let Some(adj) = adjacency.get_mut(dep_id.as_str()) {
                    adj.push(&step.id);
                }
            }
        }

        // Find all nodes with in-degree 0 and add to queue
        let mut queue: VecDeque<&str> = VecDeque::new();
        for (id, &degree) in &in_degree {
            if degree == 0 {
                queue.push_back(id);
            }
        }

        // Process queue, removing zero-in-degree nodes
        let mut processed = 0;
        while let Some(node) = queue.pop_front() {
            processed += 1;

            // For each node that depends on this one, decrement its in-degree
            if let Some(dependents) = adjacency.get(node) {
                for &dependent in dependents {
                    if let Some(degree) = in_degree.get_mut(dependent) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(dependent);
                        }
                    }
                }
            }
        }

        // If not all nodes were processed, there's a cycle
        if processed < self.steps.len() {
            // Collect nodes with non-zero in-degree
            let mut cycle_nodes: Vec<String> = in_degree
                .iter()
                .filter(|(_, &degree)| degree > 0)
                .map(|(id, _)| id.to_string())
                .collect();
            cycle_nodes.sort();
            return Err(GraphError::Cycle(cycle_nodes));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_a_dag() {
        let g = StepGraph::new(vec![
            Step::new("a", "first"),
            {
                let mut s = Step::new("b", "second");
                s.needs = vec!["a".into()];
                s
            },
        ]);
        assert!(g.validate().is_ok());
    }

    #[test]
    fn validate_rejects_cycle() {
        let g = StepGraph::new(vec![
            {
                let mut a = Step::new("a", "A");
                a.needs = vec!["b".into()];
                a
            },
            {
                let mut b = Step::new("b", "B");
                b.needs = vec!["a".into()];
                b
            },
        ]);
        assert!(matches!(g.validate(), Err(GraphError::Cycle(_))));
    }

    #[test]
    fn validate_rejects_missing_dep_and_dup() {
        let g1 = StepGraph::new(vec![{
            let mut a = Step::new("a", "A");
            a.needs = vec!["ghost".into()];
            a
        }]);
        assert!(matches!(g1.validate(), Err(GraphError::MissingDep { .. })));

        let g2 = StepGraph::new(vec![Step::new("a", "A"), Step::new("a", "dup")]);
        assert!(matches!(g2.validate(), Err(GraphError::DuplicateId(_))));
    }

    #[test]
    fn warnings_reports_unreachable_nodes() {
        let g = StepGraph::new(vec![
            {
                let mut a = Step::new("a", "A");
                a.needs = vec!["b".into()];
                a
            },
            {
                let mut b = Step::new("b", "B");
                b.needs = vec!["a".into()];
                b
            },
            Step::new("c", "C"), // independent, reachable
        ]);
        let w = g.warnings();
        // a and b are in a cycle, so unreachable; c is reachable.
        assert!(w.contains(&"a".to_string()));
        assert!(w.contains(&"b".to_string()));
        assert!(!w.contains(&"c".to_string()));
    }

    #[test]
    fn warnings_empty_for_acyclic_fully_connected() {
        let g = StepGraph::new(vec![
            Step::new("a", "first"),
            {
                let mut s = Step::new("b", "second");
                s.needs = vec!["a".into()];
                s
            },
        ]);
        assert_eq!(g.warnings(), Vec::<String>::new());
    }
}
