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
use rustc_hash::FxBuildHasher;
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

/// The builder's tables, which optimizers query millions of times. Their keys are node and
/// variable numbers the builder assigns, so a keyed hash that resists chosen collisions buys
/// nothing, and `FxHash` is several times faster.
type Table = HashMap<[i32; 3], i32, FxBuildHasher>;

#[derive(Debug, Default)]
pub(super) struct BddBuilder {
    /// `[condition, high, low]`, starting with the terminal sentinel.
    pub(super) nodes: Vec<[i32; 3]>,
    unique: Table,
    ite_cache: Table,
    /// The most nodes, and the most cached `ite` results, a build may hold.
    max_nodes: usize,
    /// Set when a build reaches `max_nodes`; its nodes are then meaningless.
    exceeded: bool,
    /// The work list and results of `ite`, which loops rather than recursing, because its
    /// depth grows with the number of conditions.
    frames: Vec<Frame>,
    values: Vec<i32>,
    /// Buffers `reduce` reuses, so repeated compiles do not reallocate them.
    spare_nodes: Vec<[i32; 3]>,
    renumbered: Vec<Option<i32>>,
}

impl BddBuilder {
    pub(super) fn new(max_nodes: usize) -> Self {
        Self {
            nodes: vec![[-1, TRUE, FALSE]],
            // Node references must stay below the result references.
            max_nodes: max_nodes.min((RESULT_OFFSET - 1) as usize),
            ..Self::default()
        }
    }

    /// Empties the builder, keeping its tables' capacity (Java's `reset`).
    pub(super) fn reset(&mut self) {
        self.nodes.clear();
        self.nodes.push([-1, TRUE, FALSE]);
        self.unique.clear();
        self.ite_cache.clear();
        self.exceeded = false;
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
            None if self.nodes.len() >= self.max_nodes => {
                self.exceeded = true;
                return TRUE;
            }
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

    /// `f` with `condition` fixed to `value`, where `condition` is at or above `f`'s top
    /// variable.
    fn cofactor(&self, f: i32, condition: i32, value: bool) -> i32 {
        if is_leaf(f) {
            return f;
        }
        let [var, high, low] = self.node(f);
        debug_assert!(var >= condition, "cofactor below the top variable");
        if var != condition {
            return f;
        }
        complement(f < 0, if value { high } else { low })
    }

    /// If `f` then `g` else `h`.
    pub(super) fn ite(&mut self, f: i32, g: i32, h: i32) -> i32 {
        let mut frames = std::mem::take(&mut self.frames);
        let mut values = std::mem::take(&mut self.values);
        frames.push(Frame::Call([f, g, h]));
        while let Some(frame) = frames.pop() {
            if self.exceeded {
                frames.clear();
                values.clear();
                values.push(TRUE);
                break;
            }
            match frame {
                Frame::Call([f, g, h]) => match self.ite_step(f, g, h) {
                    Step::Done(result) => values.push(result),
                    Step::Call(arguments) => frames.push(Frame::Call(arguments)),
                    Step::NegatedCall(arguments) => {
                        frames.extend([Frame::Negate, Frame::Call(arguments)])
                    }
                    Step::Split {
                        key,
                        var,
                        low,
                        high,
                    } => frames.extend([
                        Frame::Make { key, var },
                        Frame::Call(high),
                        Frame::Call(low),
                    ]),
                },
                Frame::Negate => {
                    let value = values.pop().expect("a call precedes its negation");
                    values.push(-value);
                }
                Frame::Make { key, var } => {
                    let high = values.pop().expect("the high call ran");
                    let low = values.pop().expect("the low call ran");
                    let result = self.make_node(var, high, low);
                    if self.ite_cache.len() >= self.max_nodes {
                        self.exceeded = true;
                    }
                    self.ite_cache.insert(key, result);
                    values.push(result);
                }
            }
        }
        let result = values.pop().expect("ite produces a value");
        debug_assert!(values.is_empty());
        (self.frames, self.values) = (frames, values);
        result
    }

    /// One call of Java's `ite`: its result, or the calls it makes.
    fn ite_step(&self, mut f: i32, mut g: i32, mut h: i32) -> Step {
        if f < 0 {
            f = -f;
            std::mem::swap(&mut g, &mut h);
        }
        if f == TRUE || g == h {
            return Step::Done(g);
        }
        if !is_result(g) && !is_result(h) {
            if g == TRUE && h == FALSE {
                return Step::Done(f);
            } else if g == FALSE && h == TRUE {
                return Step::Done(-f);
            } else if g == f {
                return Step::Call([f, TRUE, h]);
            } else if h == f {
                return Step::Call([f, g, FALSE]);
            } else if g == -f {
                return Step::Call([-f, h, FALSE]);
            } else if h == -f {
                return Step::Call([-f, TRUE, g]);
            } else if g < 0 && h < 0 {
                return Step::NegatedCall([f, -g, -h]);
            }
        }
        if let Some(cached) = self.ite_cache.get(&[f, g, h]) {
            return Step::Done(*cached);
        }
        let var = [f, g, h]
            .into_iter()
            .filter(|r| !is_leaf(*r))
            .map(|r| self.node(r)[0])
            .min()
            .expect("f is not a leaf");
        let cofactors = |value| [f, g, h].map(|r| self.cofactor(r, var, value));
        Step::Split {
            key: [f, g, h],
            var,
            low: cofactors(false),
            high: cofactors(true),
        }
    }

    /// Keeps only the nodes reachable from `root`, renumbered children first, high edges
    /// first. Returns the new root.
    pub(super) fn reduce(&mut self, root: i32) -> i32 {
        if is_leaf(root) {
            return root;
        }
        let spare = std::mem::take(&mut self.spare_nodes);
        let old = std::mem::replace(&mut self.nodes, spare);
        self.reset();
        let mut renumbered = std::mem::take(&mut self.renumbered);
        renumbered.clear();
        renumbered.resize(old.len(), None);
        let new_root = self.reduce_node(&old, &mut renumbered, root.abs());
        (self.spare_nodes, self.renumbered) = (old, renumbered);
        complement(root < 0, new_root)
    }

    /// Renumbers the node `reference` addresses and its descendants, high edges first, with
    /// an explicit stack, because the depth is the number of conditions.
    fn reduce_node(
        &mut self,
        old: &[[i32; 3]],
        renumbered: &mut [Option<i32>],
        reference: i32,
    ) -> i32 {
        let mapped = |renumbered: &[Option<i32>], reference: i32| {
            if is_leaf(reference) {
                return Some(reference);
            }
            let mapped = renumbered[reference.unsigned_abs() as usize - 1]?;
            Some(complement(reference < 0, mapped))
        };
        // A node is pushed once, then again (as `true`) once its children are pushed.
        let mut stack = vec![(reference.abs(), false)];
        while let Some((reference, children_done)) = stack.pop() {
            let index = reference as usize - 1;
            if renumbered[index].is_some() {
                continue;
            }
            let [var, high, low] = old[index];
            if children_done {
                let high = mapped(renumbered, high).expect("the high child is renumbered");
                let low = mapped(renumbered, low).expect("the low child is renumbered");
                renumbered[index] = Some(self.make_node(var, high, low));
                continue;
            }
            stack.push((reference, true));
            for child in [low, high] {
                if mapped(renumbered, child).is_none() {
                    stack.push((child.abs(), false));
                }
            }
        }
        mapped(renumbered, reference).expect("the root is renumbered")
    }
}

/// Work left in an `ite` call.
#[derive(Debug)]
enum Frame {
    /// Call `ite` and push its result.
    Call([i32; 3]),
    /// Negate the last result.
    Negate,
    /// Pop the high and low results, make their node, and cache it under `key`.
    Make { key: [i32; 3], var: i32 },
}

/// The outcome of one `ite` call before its calls run.
enum Step {
    Done(i32),
    /// The result is that of this call.
    Call([i32; 3]),
    /// The result is that of this call, negated.
    NegatedCall([i32; 3]),
    /// The result is the node testing `var` with these cofactors' results.
    Split {
        key: [i32; 3],
        var: i32,
        low: [i32; 3],
        high: [i32; 3],
    },
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
#[derive(Clone, Debug)]
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
pub(super) fn compile(
    cfg: &Cfg,
    order: Vec<usize>,
    max_nodes: usize,
) -> Result<Diagram, CompileError> {
    compile_with(&mut BddBuilder::new(max_nodes), cfg, order)
}

/// [`compile`], reusing `builder`'s allocations.
pub(super) fn compile_with(
    builder: &mut BddBuilder,
    cfg: &Cfg,
    order: Vec<usize>,
) -> Result<Diagram, CompileError> {
    builder.reset();
    let mut variables = vec![0; cfg.canonical.len()];
    for (v, &i) in order.iter().enumerate() {
        variables[i] = v as i32;
    }
    let mut results: Vec<usize> = Vec::new();
    let mut result_ids: Vec<Option<i32>> = vec![None; cfg.results.len()];
    let mut converted: Vec<Option<i32>> = vec![None; cfg.nodes.len()];
    enum Step {
        Enter(NodeId),
        Exit(NodeId),
    }
    let mut stack = vec![Step::Enter(cfg.root)];
    while let Some(step) = stack.pop() {
        match step {
            Step::Enter(id) => {
                if converted[id.index()].is_some() {
                    continue;
                }
                match cfg.node(id) {
                    Node::Result(result) => {
                        let index = match result {
                            None => 0,
                            Some(result) => *result_ids[*result].get_or_insert_with(|| {
                                results.push(*result);
                                results.len() as i32
                            }),
                        };
                        converted[id.index()] = Some(RESULT_OFFSET + index);
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
                let converted_id =
                    |id: &NodeId| converted[id.index()].expect("children exit first");
                let (mut high, mut low) = (converted_id(high), converted_id(low));
                if condition.negated {
                    std::mem::swap(&mut high, &mut low);
                }
                let test = builder.make_node(variables[condition.index], TRUE, FALSE);
                let node = builder.ite(test, high, low);
                converted[id.index()] = Some(node);
            }
        }
        if builder.exceeded {
            return Err(CompileError::limit(format!(
                "building the BDD needs more than {} nodes",
                builder.max_nodes
            )));
        }
    }
    let root = builder.reduce(converted[cfg.root.index()].expect("the root is converted"));
    Ok(Diagram {
        root,
        nodes: builder.nodes[1..].to_vec(),
        conditions: order,
        results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `x0 ∧ x2 ∧ …` (or odd variables, from `start = 1`) below `n`, built bottom-up.
    fn chain(builder: &mut BddBuilder, start: i32, n: i32) -> i32 {
        (start..n)
            .step_by(2)
            .rev()
            .fold(TRUE, |rest, v| builder.make_node(v, rest, FALSE))
    }

    /// Conjoining two interleaved chains makes `ite` and `reduce` descend once per variable,
    /// which would overflow this thread's stack if they recursed.
    #[test]
    fn deep_operations_do_not_recurse() {
        const N: i32 = 100_000;
        let run = || {
            let mut builder = BddBuilder::new(usize::MAX);
            let (f, g) = (chain(&mut builder, 0, N), chain(&mut builder, 1, N));
            let both = builder.ite(f, g, FALSE);
            builder.reduce(both);
            builder.nodes.len()
        };
        let thread = std::thread::Builder::new().stack_size(128 * 1024);
        let nodes = thread.spawn(run).unwrap().join().unwrap();
        assert_eq!(nodes, N as usize + 1);
    }

    #[test]
    fn stops_at_the_node_limit() {
        let mut builder = BddBuilder::new(1_000);
        let (f, g) = (chain(&mut builder, 0, 600), chain(&mut builder, 1, 600));
        assert!(!builder.exceeded);
        builder.ite(f, g, FALSE);
        assert!(builder.exceeded);
        assert_eq!(builder.nodes.len(), 1_000);
        builder.reset();
        assert!(!builder.exceeded);
    }
}
