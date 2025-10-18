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

//TODO(bdd): Should probably make P, C, and R statically typed, but need to write those types first
/// Evaluates a BDD to resolve an endpoint result
///
/// # Arguments
/// * `nodes` - Array of BDD nodes
/// * `root_ref` - Root reference to start evaluation
/// * `params` - Parameters for condition evaluation
/// * `conditions` - Array of conditions referenced by nodes
/// * `results` - Array of possible results
/// * `condition_evaluator` - Function to evaluate conditions
///
/// # Returns
/// * `Some(&R)` - Result if evaluation succeeds
/// * `None` - No match found (terminal reached)
pub fn evaluate_bdd<P, C, R: Clone>(
    nodes: &[BddNode],
    root_ref: i32,
    params: &P,
    conditions: &[C],
    results: &[R],
    condition_evaluator: impl Fn(&P, &C) -> bool,
) -> Option<R> {
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
                let condition_result = condition_evaluator(params, condition);

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
