/*
 *  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *  SPDX-License-Identifier: Apache-2.0
 */

use crate::endpoint_lib::parse_url::Url;
use std::collections::HashMap;

//TODO(bdd): Should this just be an [i32; 3]? Might make it easier to make things const?
/// Binary Decision Diagram node representation
#[derive(Debug, Clone, Copy)]
pub struct BddNode {
    pub condition_index: i32,
    pub high_ref: i32,
    pub low_ref: i32,
}

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
pub fn evaluate_bdd<'a, Cond, Params, Res: Clone, Context>(
    nodes: &[BddNode],
    conditions: &[Cond],
    results: &[Res],
    root_ref: i32,
    params: &'a Params,
    context: &mut Context,
    diagnostic_collector: &mut crate::endpoint_lib::diagnostic::DiagnosticCollector,
    mut condition_evaluator: impl FnMut(
        &Cond,
        &'a Params,
        &mut Context,
        &mut crate::endpoint_lib::diagnostic::DiagnosticCollector,
    ) -> bool,
) -> Option<Res> {
    let mut current_ref = root_ref;

    loop {
        match current_ref {
            // Result references (>= 100_000_000)
            ref_val if ref_val >= 100_000_000 => {
                let result_index = (ref_val - 100_000_000) as usize;
                return results.get(result_index).map(Clone::clone);
            }
            // Terminals (1 = TRUE, -1 = FALSE) NoMatchRule
            1 | -1 => return None, //TODO(BDD) should probably be results.get(0)?, but need to figure out the NoMatchRule thing
            // Node references
            ref_val => {
                let is_complement = ref_val < 0;
                let node_index = (ref_val.abs() - 1) as usize;

                let node = nodes.get(node_index)?;
                let condition_index = node.condition_index as usize;
                let condition = conditions.get(condition_index)?;
                let condition_result =
                    condition_evaluator(condition, params, context, diagnostic_collector);

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
