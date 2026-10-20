/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Condition dependencies (Java's `ConditionDependencyGraph`) and the initial variable order
//! (`InitialOrdering` with `CfgConeAnalysis`).

use super::cfg::{Cfg, Node, NodeId};
use super::syntax::reference_set;
use crate::endpoint_rules::language::Condition;
use std::collections::{BTreeSet, HashMap};

/// Which conditions must be tested before which: a condition depends on the conditions that
/// assign the names it uses and, unless it is an `isSet` itself, on the `isSet` conditions
/// that test them.
#[derive(Debug)]
pub(super) struct Dependencies {
    pub(super) predecessors: Vec<BTreeSet<usize>>,
    pub(super) successors: Vec<BTreeSet<usize>>,
}

impl Dependencies {
    pub(super) fn new(conditions: &[&Condition]) -> Self {
        let n = conditions.len();
        let mut producers: HashMap<&str, Vec<usize>> = HashMap::new();
        let mut is_setters: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, condition) in conditions.iter().enumerate() {
            if let Some(name) = condition.assign() {
                producers.entry(name).or_default().push(i);
            }
            if condition.function_name() == "isSet" {
                for name in reference_set(&condition.function) {
                    is_setters.entry(name).or_default().push(i);
                }
            }
        }
        let mut predecessors = vec![BTreeSet::new(); n];
        let mut successors = vec![BTreeSet::new(); n];
        for (i, condition) in conditions.iter().enumerate() {
            let is_set = condition.function_name() == "isSet";
            for name in reference_set(&condition.function) {
                let setters = (!is_set).then(|| is_setters.get(name)).flatten();
                for &before in producers.get(name).into_iter().chain(setters).flatten() {
                    if before != i {
                        predecessors[i].insert(before);
                        successors[before].insert(i);
                    }
                }
            }
        }
        Self {
            predecessors,
            successors,
        }
    }
}

/// Java's `InitialOrdering`: a topological order of `conditions` (canonical indexes in CFG
/// order) that prefers conditions closest to the CFG root, then `isSet` gates with more than
/// two dependents, then conditions leading to more results, then CFG order.
pub(super) fn initial_order(cfg: &Cfg, conditions: &[usize]) -> Vec<usize> {
    let refs: Vec<_> = conditions.iter().map(|&i| &cfg.canonical[i]).collect();
    let dependencies = Dependencies::new(&refs);
    let cones = Cones::new(cfg, conditions);
    let is_gate =
        |i: usize| refs[i].function_name() == "isSet" && dependencies.successors[i].len() > 2;

    let n = conditions.len();
    let mut placed = vec![false; n];
    let mut ready: BTreeSet<usize> = (0..n)
        .filter(|&i| dependencies.predecessors[i].is_empty())
        .collect();
    let mut order = Vec::with_capacity(n);
    while !ready.is_empty() {
        // (index, depth, cone, gate) of the best ready condition, as Java compares them.
        let mut best: Option<(usize, usize, i32, bool)> = None;
        for &i in &ready {
            let (depth, cone, gate) = (cones.depth[i], cones.results[i], is_gate(i));
            let better = match best {
                None => true,
                Some((_, best_depth, best_cone, best_gate)) => {
                    depth < best_depth
                        || depth == best_depth
                            && ((!best_gate && gate) || (best_gate == gate && cone > best_cone))
                }
            };
            if better {
                best = Some((i, depth, cone, gate));
            }
        }
        let (chosen, ..) = best.expect("ready is not empty");
        order.push(chosen);
        placed[chosen] = true;
        ready.remove(&chosen);
        for &next in &dependencies.successors[chosen] {
            if !placed[next] && dependencies.predecessors[next].iter().all(|&p| placed[p]) {
                ready.insert(next);
            }
        }
    }
    debug_assert_eq!(order.len(), n, "SSA conditions have no dependency cycles");
    order.into_iter().map(|i| conditions[i]).collect()
}

/// Java's `CfgConeAnalysis`, per condition position: the least depth at which the CFG first
/// reaches it, and the most results (counted per path) below a node testing it.
///
/// Path counts grow exponentially; like Java's `int`s, they wrap.
struct Cones {
    depth: Vec<usize>,
    results: Vec<i32>,
}

impl Cones {
    fn new(cfg: &Cfg, conditions: &[usize]) -> Self {
        let positions: HashMap<usize, usize> = conditions
            .iter()
            .enumerate()
            .map(|(p, &i)| (i, p))
            .collect();
        let mut depth = vec![usize::MAX; conditions.len()];
        let mut results = vec![0; conditions.len()];
        // Results below each finished node. Like Java, a node is analyzed once, at the depth
        // it is first reached, high branch first.
        let mut cones: HashMap<NodeId, i32> = HashMap::new();
        enum Step {
            Enter(NodeId, usize),
            Exit(NodeId),
        }
        let mut stack = vec![Step::Enter(cfg.root, 0)];
        while let Some(step) = stack.pop() {
            match step {
                Step::Enter(id, d) => {
                    if cones.contains_key(&id) {
                        continue;
                    }
                    match cfg.node(id) {
                        Node::Result(_) => {
                            cones.insert(id, 1);
                        }
                        Node::Condition {
                            condition,
                            high,
                            low,
                        } => {
                            let p = positions[&condition.index];
                            depth[p] = depth[p].min(d);
                            stack.extend([Step::Exit(id), Step::Enter(*low, d + 1)]);
                            stack.push(Step::Enter(*high, d + 1));
                        }
                    }
                }
                Step::Exit(id) => {
                    let Node::Condition {
                        condition,
                        high,
                        low,
                    } = cfg.node(id)
                    else {
                        unreachable!("only condition nodes exit")
                    };
                    let cone = cones[high].wrapping_add(cones[low]);
                    let p = positions[&condition.index];
                    results[p] = results[p].max(cone);
                    cones.insert(id, cone);
                }
            }
        }
        Self { depth, results }
    }
}
