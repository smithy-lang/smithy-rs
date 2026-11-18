/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! How expensive a BDD is expected to be to evaluate: Java's `ConditionCostModel` (default)
//! and `BddCostEstimator`, with every condition equally likely to hold.
//!
//! Optimizers break ties by comparing these costs exactly, so the arithmetic follows Java's:
//! `int` costs that wrap, and `double` sums in Java's order.

use super::builder::{Diagram, RESULT_OFFSET};
use super::functions;
use crate::endpoint_rules::language::{Condition, Expression, ExpressionKind};

/// How much more a nested function call costs than the same call at the top level.
const NESTED_MULTIPLIER: i32 = 2;

/// Java's default `ConditionCostModel`: the function's cost plus twice the cost of each
/// argument that is itself a function call.
pub(super) fn condition_cost(condition: &Condition) -> i32 {
    fn cost(expression: &Expression) -> i32 {
        let ExpressionKind::Function { name, arguments } = &expression.kind else {
            return 0;
        };
        let base = functions::get(name).map_or(100, |f| f.cost);
        arguments
            .iter()
            .filter(|a| matches!(a.kind, ExpressionKind::Function { .. }))
            .fold(base, |total, a| {
                total.wrapping_add(cost(a).wrapping_mul(NESTED_MULTIPLIER))
            })
    }
    cost(&condition.function)
}

fn is_node(reference: i32) -> bool {
    (reference > 1 && reference < RESULT_OFFSET) || (reference < -1 && reference > -RESULT_OFFSET)
}

/// Java's `BddCostEstimator.expectedCost`: the sum over variables of how often each is
/// expected to be tested times its cost, where `costs[v]` is variable `v`'s cost.
pub(super) fn expected_cost(diagram: &Diagram, costs: &[i32]) -> f64 {
    if !is_node(diagram.root) {
        return 0.0;
    }
    expected_evaluations(diagram, costs.len())
        .iter()
        .zip(costs)
        .map(|(&e, &c)| e * f64::from(c))
        .fold(0.0, |total, x| total + x)
}

/// Java's `BddCostEstimator.reachProbabilities`: how often each of the `variables` is
/// expected to be tested.
///
/// Like Java, this walks nodes from the last stored to the first, which visits parents before
/// children in the order `BddBuilder` stores them.
pub(super) fn expected_evaluations(diagram: &Diagram, variables: usize) -> Vec<f64> {
    let mut evaluations = vec![0.0f64; variables];
    if !is_node(diagram.root) {
        return evaluations;
    }
    let nodes = &diagram.nodes;
    // Java indexes nodes from the sentinel, so its node `i` is `nodes[i - 1]`.
    let mut reach = vec![0.0f64; nodes.len()];
    reach[diagram.root.unsigned_abs() as usize - 2] = 1.0;
    for i in (0..nodes.len()).rev() {
        let probability = reach[i];
        if probability < 1e-9 {
            continue;
        }
        let [variable, high, low] = nodes[i];
        evaluations[variable as usize] += probability;
        let p_true = 0.5;
        if is_node(high) {
            reach[high.unsigned_abs() as usize - 2] += probability * p_true;
        }
        if is_node(low) {
            reach[low.unsigned_abs() as usize - 2] += probability * (1.0 - p_true);
        }
    }
    evaluations
}
