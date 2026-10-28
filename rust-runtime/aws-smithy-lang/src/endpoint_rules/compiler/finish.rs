/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The last two stages: Java's `NodeReversal`, which stores nodes parents first, and
//! `EndpointBddTrait.removeUnreferencedConditions`.

use super::builder::{Diagram, RESULT_OFFSET};
use super::cfg::Cfg;
use super::syntax::{as_reference, literal_references, references};
use crate::endpoint_rules::language::ResultKind;
use std::collections::BTreeSet;

/// Java's `NodeReversal`: reverses the order nodes are stored in, so that a parent precedes
/// its children.
pub(super) fn reverse(diagram: Diagram) -> Diagram {
    let n = diagram.nodes.len();
    // Java leaves a BDD of at most one node as it is.
    if n <= 1 {
        return diagram;
    }
    // Node references are `index + 2`; node `i` moves to `n - 1 - i`.
    let remap = |reference: i32| {
        if reference.abs() <= 1 || reference >= RESULT_OFFSET {
            reference
        } else {
            reference.signum() * (n as i32 + 3 - reference.abs())
        }
    };
    Diagram {
        root: remap(diagram.root),
        nodes: diagram
            .nodes
            .iter()
            .rev()
            .map(|&[variable, high, low]| [variable, remap(high), remap(low)])
            .collect(),
        ..diagram
    }
}

/// `removeUnreferencedConditions`: drops conditions no node tests, renumbering variables.
///
/// Java also drops an untested condition that an (unreachable) result or a kept condition
/// relies on: one whose assignment it uses, or an `isSet` of a name it uses. The BDD must
/// still type-check, so this keeps those.
pub(super) fn remove_unreferenced_conditions(cfg: &Cfg, diagram: Diagram) -> Diagram {
    let mut kept = vec![false; diagram.conditions.len()];
    for node in &diagram.nodes {
        kept[node[0] as usize] = true;
    }
    let mut needed = BTreeSet::new();
    for &result in &diagram.results {
        match &cfg.results[result].kind {
            ResultKind::Endpoint(endpoint) => {
                references(&endpoint.url, &mut needed);
                for (_, values) in endpoint.headers.iter() {
                    values.iter().for_each(|v| references(v, &mut needed));
                }
                for (_, value) in endpoint.properties.iter() {
                    literal_references(value, &mut needed);
                }
            }
            ResultKind::Error(error) => references(error, &mut needed),
        }
    }
    // Conditions only rely on earlier ones, so one backward pass finds all that are needed.
    for (i, &index) in diagram.conditions.iter().enumerate().rev() {
        let condition = &cfg.canonical[index];
        let proves_set = condition.function_name() == "isSet"
            && condition
                .arguments()
                .first()
                .and_then(as_reference)
                .is_some_and(|name| needed.contains(name));
        if condition.assign().is_some_and(|name| needed.contains(name)) || proves_set {
            kept[i] = true;
        }
        if kept[i] {
            references(&condition.function, &mut needed);
        }
    }
    if kept.iter().all(|&k| k) {
        return diagram;
    }
    let mut renumbered = vec![-1; kept.len()];
    let mut conditions = Vec::new();
    for (i, &index) in diagram.conditions.iter().enumerate() {
        if kept[i] {
            renumbered[i] = conditions.len() as i32;
            conditions.push(index);
        }
    }
    Diagram {
        nodes: diagram
            .nodes
            .iter()
            .map(|&[variable, high, low]| [renumbered[variable as usize], high, low])
            .collect(),
        conditions,
        ..diagram
    }
}
