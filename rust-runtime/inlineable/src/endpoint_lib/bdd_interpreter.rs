/*
 *  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *  SPDX-License-Identifier: Apache-2.0
 */

//TODO(bdd): Should this just be an [i32; 3]? Might make it easier to make things const?
/// Binary Decision Diagram node representation
#[derive(Debug, Clone, Copy)]
pub struct BddNode {
    pub condition_index: i32,
    pub high_ref: i32,
    pub low_ref: i32,
}

/// Intermediate result from a condition evaluation
#[derive(Debug, Clone)]
pub enum ConditionResult {
    String(String),
}

/// Stores intermediate results from condition evaluations
#[derive(Debug, Default)]
pub struct ConditionContext {
    results: Vec<Option<ConditionResult>>,
}

impl ConditionContext {
    pub fn new(condition_count: usize) -> Self {
        Self {
            results: vec![None; condition_count],
        }
    }

    pub fn store(&mut self, index: usize, result: ConditionResult) {
        if let Some(slot) = self.results.get_mut(index) {
            *slot = Some(result);
        }
    }

    pub fn get(&self, index: usize) -> Option<&ConditionResult> {
        self.results.get(index)?.as_ref()
    }

    pub fn get_string(&self, index: usize) -> Option<&str> {
        match self.get(index)? {
            ConditionResult::String(s) => Some(s.as_str()),
        }
    }
}

//TODO(bdd): Should probably make P, C, and R statically typed, but need to write those types first
/// Evaluates a BDD to resolve an endpoint result
///
/// Arguments
/// * `nodes` - Array of BDD nodes
/// * `root_ref` - Root reference to start evaluation
/// * `params` - Parameters for condition evaluation
/// * `conditions` - Array of conditions referenced by nodes
/// * `results` - Array of possible results
/// * `condition_evaluator` - Function to evaluate conditions with context
///
/// Returns
/// * `Some(&R)` - Result if evaluation succeeds
/// * `None` - No match found (terminal reached)
pub fn evaluate_bdd<P, C, R: Clone>(
    nodes: &[BddNode],
    root_ref: i32,
    params: &P,
    conditions: &[C],
    results: &[R],
    diagnostic_collector: &mut crate::endpoint_lib::diagnostic::DiagnosticCollector,
    condition_evaluator: impl Fn(
        &P,
        &C,
        &mut crate::endpoint_lib::diagnostic::DiagnosticCollector,
        &mut ConditionContext,
    ) -> bool,
) -> Option<R> {
    let mut context = ConditionContext::new(conditions.len());
    let mut current_ref = root_ref;

    loop {
        match current_ref {
            // Result references (>= 100_000_000)
            ref_val if ref_val >= 100_000_000 => {
                let result_index = (ref_val - 100_000_000) as usize;
                return results.get(result_index).map(Clone::clone);
            }
            // Terminals (1 = TRUE, -1 = FALSE) - no match
            1 | -1 => return None,
            // Node references
            ref_val => {
                let is_complement = ref_val < 0;
                let node_index = (ref_val.abs() - 2) as usize;

                let node = nodes.get(node_index)?;
                let condition = conditions.get(node.condition_index as usize)?;
                let condition_result =
                    condition_evaluator(params, condition, diagnostic_collector, &mut context);

                // Handle complement edges: complement inverts the branch selection
                current_ref = if is_complement ^ condition_result {
                    node.high_ref
                } else {
                    node.low_ref
                };
            }
        }
    }
}
