/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Java's `BddBuilder` and `BddCompiler`: builds a reduced, ordered BDD with complement edges
//! from the CFG.
//!
//! References use Java's encoding, which is also the wire encoding: `1` and `-1` are the
//! terminals, `n + 1` is node `n` (node 0 is the terminal sentinel) and `-(n + 1)` its
//! complement, and `RESULT_OFFSET + i` is result `i`.

use super::cfg::{Cfg, Node, NodeId};
use super::CompileError;
use std::collections::HashMap;

pub(super) const TRUE: i32 = 1;
pub(super) const FALSE: i32 = -1;
pub(super) const RESULT_OFFSET: i32 = 100_000_000;

pub(super) fn is_leaf(reference: i32) -> bool {
    reference.abs() == TRUE || reference >= RESULT_OFFSET
}

fn is_result(reference: i32) -> bool {
    reference >= RESULT_OFFSET
}

#[derive(Debug, Default)]
pub(super) struct BddBuilder {
    /// `[condition, high, low]`, starting with the terminal sentinel.
    pub(super) nodes: Vec<[i32; 3]>,
    unique: HashMap<[i32; 3], i32>,
    ite_cache: HashMap<[i32; 3], i32>,
}

impl BddBuilder {
    pub(super) fn new() -> Self {
        Self {
            nodes: vec![[-1, TRUE, FALSE]],
            ..Self::default()
        }
    }

    /// The node `reference` addresses, which must not be a leaf.
    fn node(&self, reference: i32) -> [i32; 3] {
        self.nodes[reference.unsigned_abs() as usize - 1]
    }

    pub(super) fn make_node(&mut self, condition: i32, high: i32, low: i32) -> i32 {
        if high == low {
            return high;
        }
        // Canonical form: the low edge is never complemented.
        let flip = should_flip(high, low);
        let key = if flip {
            [condition, -high, -low]
        } else {
            [condition, high, low]
        };
        let index = match self.unique.get(&key) {
            Some(index) => *index,
            None => {
                let index = self.nodes.len() as i32;
                self.nodes.push(key);
                self.unique.insert(key, index);
                index
            }
        };
        if flip {
            -(index + 1)
        } else {
            index + 1
        }
    }

    /// `f` with `condition` fixed to `value`.
    fn cofactor(&mut self, f: i32, condition: i32, value: bool) -> i32 {
        if is_leaf(f) {
            return f;
        }
        let [var, high, low] = self.node(f);
        let result = if var == condition {
            if value {
                high
            } else {
                low
            }
        } else if var > condition {
            return f;
        } else {
            let high = self.cofactor(high, condition, value);
            let low = self.cofactor(low, condition, value);
            self.make_node(var, high, low)
        };
        complement(f < 0, result)
    }

    /// If `f` then `g` else `h`.
    pub(super) fn ite(&mut self, mut f: i32, mut g: i32, mut h: i32) -> i32 {
        if f < 0 {
            f = -f;
            std::mem::swap(&mut g, &mut h);
        }
        if f == TRUE || g == h {
            return g;
        }
        if !is_result(g) && !is_result(h) {
            if g == TRUE && h == FALSE {
                return f;
            } else if g == FALSE && h == TRUE {
                return -f;
            } else if g == f {
                return self.ite(f, TRUE, h);
            } else if h == f {
                return self.ite(f, g, FALSE);
            } else if g == -f {
                return self.ite(-f, h, FALSE);
            } else if h == -f {
                return self.ite(-f, TRUE, g);
            } else if g < 0 && h < 0 {
                return -self.ite(f, -g, -h);
            }
        }
        if let Some(cached) = self.ite_cache.get(&[f, g, h]) {
            return *cached;
        }
        let var = [f, g, h]
            .into_iter()
            .filter(|r| !is_leaf(*r))
            .map(|r| self.node(r)[0])
            .min()
            .expect("f is not a leaf");
        let (f0, g0, h0) = (
            self.cofactor(f, var, false),
            self.cofactor(g, var, false),
            self.cofactor(h, var, false),
        );
        let low = self.ite(f0, g0, h0);
        let (f1, g1, h1) = (
            self.cofactor(f, var, true),
            self.cofactor(g, var, true),
            self.cofactor(h, var, true),
        );
        let high = self.ite(f1, g1, h1);
        let result = self.make_node(var, high, low);
        self.ite_cache.insert([f, g, h], result);
        result
    }

    /// Keeps only the nodes reachable from `root`, renumbered children first, high edges
    /// first. Returns the new root.
    pub(super) fn reduce(&mut self, root: i32) -> i32 {
        if is_leaf(root) {
            return root;
        }
        let old = std::mem::replace(&mut self.nodes, vec![[-1, TRUE, FALSE]]);
        self.unique.clear();
        self.ite_cache.clear();
        let mut renumbered = vec![None; old.len()];
        let new_root = self.reduce_node(&old, &mut renumbered, root.abs());
        complement(root < 0, new_root)
    }

    fn reduce_node(
        &mut self,
        old: &[[i32; 3]],
        renumbered: &mut [Option<i32>],
        reference: i32,
    ) -> i32 {
        if is_leaf(reference) {
            return reference;
        }
        let index = reference.unsigned_abs() as usize - 1;
        let mapped = match renumbered[index] {
            Some(mapped) => mapped,
            None => {
                let [var, high, low] = old[index];
                let high = self.reduce_node(old, renumbered, high);
                let low = self.reduce_node(old, renumbered, low);
                let mapped = self.make_node(var, high, low);
                renumbered[index] = Some(mapped);
                mapped
            }
        };
        complement(reference < 0, mapped)
    }
}

fn should_flip(high: i32, low: i32) -> bool {
    low < 0 && !is_result(high) && !is_result(low)
}

fn complement(complemented: bool, reference: i32) -> i32 {
    debug_assert!(
        !(complemented && is_result(reference)),
        "complemented result"
    );
    if complemented {
        -reference
    } else {
        reference
    }
}

/// A BDD in Java's encoding, with conditions and results as indexes into the CFG's.
#[derive(Debug)]
pub(super) struct Diagram {
    pub(super) root: i32,
    /// Decision nodes, after the sentinel.
    pub(super) nodes: Vec<[i32; 3]>,
    /// Canonical condition indexes, in variable order.
    pub(super) conditions: Vec<usize>,
    /// CFG result indexes, in result order after the implicit no-match result 0.
    pub(super) results: Vec<usize>,
}

/// Java's `BddCompiler.compile`: converts the CFG with conditions in `order` (canonical
/// indexes), numbering results as they are first reached, high branches first.
pub(super) fn compile(cfg: &Cfg, order: Vec<usize>) -> Result<Diagram, CompileError> {
    let variables: HashMap<usize, i32> = order
        .iter()
        .enumerate()
        .map(|(v, &i)| (i, v as i32))
        .collect();
    let mut builder = BddBuilder::new();
    let mut results: Vec<usize> = Vec::new();
    let mut result_ids: HashMap<usize, i32> = HashMap::new();
    let mut converted: HashMap<NodeId, i32> = HashMap::new();
    enum Step {
        Enter(NodeId),
        Exit(NodeId),
    }
    let mut stack = vec![Step::Enter(cfg.root)];
    while let Some(step) = stack.pop() {
        match step {
            Step::Enter(id) => {
                if converted.contains_key(&id) {
                    continue;
                }
                match cfg.node(id) {
                    Node::Result(result) => {
                        let index = match result {
                            None => 0,
                            Some(result) => *result_ids.entry(*result).or_insert_with(|| {
                                results.push(*result);
                                results.len() as i32
                            }),
                        };
                        converted.insert(id, RESULT_OFFSET + index);
                    }
                    Node::Condition { high, low, .. } => {
                        stack.extend([Step::Exit(id), Step::Enter(*low), Step::Enter(*high)]);
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
                let (mut high, mut low) = (converted[high], converted[low]);
                if condition.negated {
                    std::mem::swap(&mut high, &mut low);
                }
                let test = builder.make_node(variables[&condition.index], TRUE, FALSE);
                let node = builder.ite(test, high, low);
                converted.insert(id, node);
            }
        }
        if builder.nodes.len() >= (RESULT_OFFSET - 1) as usize {
            return Err(CompileError::new("the BDD has too many nodes"));
        }
    }
    let root = builder.reduce(converted[&cfg.root]);
    Ok(Diagram {
        root,
        nodes: builder.nodes.split_off(1),
        conditions: order,
        results,
    })
}
