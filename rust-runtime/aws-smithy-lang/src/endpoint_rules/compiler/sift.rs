/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Java's `SiftingOptimization`: reorders conditions to shrink the BDD, recompiling the CFG for
//! each candidate order.
//!
//! The search is Java's, step for step, so it ends at the same order: which conditions it
//! tries to move, to which positions, and how it breaks ties (size, then exact expected cost,
//! then position). Like Java, it compiles a condition's candidate positions in parallel (see
//! [`search`](super::search)).

use super::builder::Diagram;
use super::cfg::Cfg;
use super::cost::{condition_cost, expected_cost};
use super::order::{Constraints, DependencyGraph};
use super::search::{move_to, with_compiler, Compiler};
use super::CompileError;
use std::cmp::Ordering::Less;

/// The passes of one effort level (Java's `OptimizationEffort`).
#[derive(Clone, Copy)]
struct Effort {
    sample_rate: i32,
    max_positions: i32,
    nearby_radius: i32,
    exhaustive_threshold: isize,
}

const COARSE: Effort = Effort {
    sample_rate: 11,
    max_positions: 4,
    nearby_radius: 0,
    exhaustive_threshold: 20,
};
const MEDIUM: Effort = Effort {
    sample_rate: 2,
    max_positions: 24,
    nearby_radius: 6,
    exhaustive_threshold: 20,
};
const GRANULAR: Effort = Effort {
    sample_rate: 1,
    max_positions: 50,
    nearby_radius: 12,
    exhaustive_threshold: 20,
};

/// Java's defaults: coarse passes run above 4,000 nodes, medium above 1,000, and granular
/// passes, block moves, and adjacent swaps at or below 8,000.
const COARSE_MIN_NODES: usize = 4_000;
const COARSE_MAX_PASSES: i32 = 6;
const MEDIUM_MIN_NODES: usize = 1_000;
const MEDIUM_MAX_PASSES: i32 = 6;
const GRANULAR_MAX_NODES: usize = 8_000;
const GRANULAR_MAX_PASSES: i32 = 12;

const PLATEAU_HISTORY: usize = 3;
const PLATEAU_THRESHOLD: f64 = 0.5;

/// Java's `AdaptiveEffort`: searches harder after a pass that reduces size by at least 10%,
/// and less after one that reduces it by under 2%.
struct Adaptive {
    base: Effort,
    current: Effort,
    bonus_passes: i32,
}

impl Adaptive {
    fn new(base: Effort) -> Self {
        Self {
            base,
            current: base,
            bonus_passes: 0,
        }
    }

    fn adapt(&mut self, reduction: f64) -> bool {
        let (base, e) = (self.base, &mut self.current);
        if reduction >= 10.0 {
            e.sample_rate = (e.sample_rate - 1).max(1);
            e.max_positions = (base.max_positions * 2).min(e.max_positions + 5);
            e.nearby_radius = (base.nearby_radius + 6).min(e.nearby_radius + 2);
            self.bonus_passes = (self.bonus_passes + 2).min(6);
            true
        } else {
            if reduction < 2.0 {
                e.sample_rate = (base.sample_rate * 2).min(e.sample_rate + 2);
                e.max_positions = (base.max_positions / 2).max(e.max_positions - 3);
                e.nearby_radius = (e.nearby_radius - 2).max(0);
                self.bonus_passes = (self.bonus_passes - 2).max(0);
            }
            false
        }
    }
}

/// The order being optimized and the smallest BDD found for it.
struct State {
    order: Vec<usize>,
    best: Diagram,
}

impl State {
    fn size(&self) -> usize {
        self.best.nodes.len()
    }
}

/// A candidate order's BDD.
struct Candidate {
    position: isize,
    diagram: Diagram,
    cost: f64,
}

struct Sifter<'a> {
    compiler: &'a Compiler<'a>,
    graph: DependencyGraph,
    /// The cost of each canonical condition.
    costs: Vec<i32>,
}

/// Sifts `diagram`, returning it unchanged unless a smaller BDD is found.
pub(super) fn sift(cfg: &Cfg, diagram: Diagram, max_nodes: usize) -> Result<Diagram, CompileError> {
    if diagram.conditions.is_empty() {
        return Ok(diagram);
    }
    with_compiler(cfg, diagram.conditions.len(), max_nodes, |compiler| {
        Sifter {
            compiler,
            graph: DependencyGraph::new(cfg),
            costs: cfg.canonical.iter().map(condition_cost).collect(),
        }
        .run(diagram)
    })
}

impl Sifter<'_> {
    fn run(&self, diagram: Diagram) -> Result<Diagram, CompileError> {
        let initial_size = diagram.nodes.len();
        let mut state = State {
            order: diagram.conditions.clone(),
            best: diagram.clone(),
        };
        self.stage(&mut state, COARSE, COARSE_MIN_NODES, COARSE_MAX_PASSES, 4.0)?;
        self.stage(&mut state, MEDIUM, MEDIUM_MIN_NODES, MEDIUM_MAX_PASSES, 1.5)?;
        if state.size() <= GRANULAR_MAX_NODES {
            self.stage(&mut state, GRANULAR, 0, GRANULAR_MAX_PASSES, 0.0)?;
        }
        self.block_moves(&mut state)?;
        self.adjacent_swaps(&mut state)?;
        debug_assert!(state.best.conditions == state.order);
        // Like Java, keep the input unless the BDD shrank: passes also accept moves that keep
        // its size.
        Ok(if state.size() < initial_size {
            state.best
        } else {
            diagram
        })
    }

    fn compile(&self, order: &[usize]) -> Result<Diagram, CompileError> {
        self.compiler.compile(order)
    }

    fn cost(&self, diagram: &Diagram) -> f64 {
        let costs: Vec<i32> = diagram.conditions.iter().map(|&c| self.costs[c]).collect();
        expected_cost(diagram, &costs)
    }

    /// Java's `runOptimizationStage`: up to `max_passes` passes (plus bonus passes), until
    /// the BDD has at most `target` nodes, a pass changes nothing, or reductions level off.
    fn stage(
        &self,
        state: &mut State,
        effort: Effort,
        target: usize,
        max_passes: i32,
        min_reduction: f64,
    ) -> Result<(), CompileError> {
        if target > 0 && state.size() <= target {
            return Ok(());
        }
        let mut adaptive = Adaptive::new(effort);
        let mut history = [0.0f64; PLATEAU_HISTORY];
        let (mut passes, mut consecutive_low) = (0usize, 0);
        let mut pass = 1;
        while pass <= max_passes + adaptive.bonus_passes {
            if target > 0 && state.size() <= target {
                break;
            }
            let start = state.size();
            if !self.pass(state, &adaptive.current)? {
                break;
            }
            let reduction = (1.0 - state.size() as f64 / start as f64) * 100.0;
            history[passes % PLATEAU_HISTORY] = reduction;
            passes += 1;
            // A reduction is NaN for an empty BDD, which Java counts as low.
            let low = |r: f64| matches!(r.partial_cmp(&PLATEAU_THRESHOLD), Some(Less) | None);
            if passes >= PLATEAU_HISTORY && history.iter().all(|&r| low(r)) {
                break;
            }
            consecutive_low = if adaptive.adapt(reduction) {
                0
            } else if reduction < 2.0 {
                consecutive_low + 1
            } else {
                0
            };
            if consecutive_low >= 2 || (min_reduction > 0.0 && reduction < min_reduction) {
                break;
            }
            pass += 1;
        }
        Ok(())
    }

    /// Java's `runPass`: moves each sampled condition, most-tested first, to its best position
    /// if the BDD does not grow. Returns whether any condition moved.
    fn pass(&self, state: &mut State, effort: &Effort) -> Result<bool, CompileError> {
        let n = state.order.len();
        let mut counts = vec![0usize; n];
        for node in &state.best.nodes {
            counts[node[0] as usize] += 1;
        }
        // Positions are sampled at the start of the pass; like Java's, they are positions,
        // not conditions, though earlier moves in the pass shift conditions between them.
        let mut selected: Vec<usize> = (0..n).collect();
        selected.sort_by(|&a, &b| counts[b].cmp(&counts[a]));
        if effort.sample_rate > 1 {
            selected.truncate((n / effort.sample_rate as usize).max(1));
        }
        let mut constraints = self.graph.constraints(&state.order);
        let mut moved = false;
        for variable in selected {
            let positions = strategic_positions(variable, &constraints, effort, n);
            if positions.is_empty() {
                continue;
            }
            if let Some(best) = self.best_position(&positions, state, variable)? {
                move_to(&mut state.order, variable, best.position as usize);
                state.best = best.diagram;
                constraints = self.graph.constraints(&state.order);
                moved = true;
            }
        }
        Ok(moved)
    }

    /// Java's `findBestPosition`: of the moves that do not grow the BDD, the smallest, then
    /// cheapest, then earliest.
    fn best_position(
        &self,
        positions: &[usize],
        state: &State,
        variable: usize,
    ) -> Result<Option<Candidate>, CompileError> {
        let moves: Vec<_> = positions.iter().map(|&p| (variable, p)).collect();
        let candidates = (self.compiler).compile_moves(&state.order, &moves, state.size())?;
        let Some(min) = candidates.iter().map(|(_, d)| d.nodes.len()).min() else {
            return Ok(None);
        };
        let mut best: Option<Candidate> = None;
        for (i, diagram) in candidates {
            if diagram.nodes.len() != min {
                continue;
            }
            let cost = self.cost(&diagram);
            let position = positions[i] as isize;
            if best
                .as_ref()
                .is_none_or(|b| cost < b.cost || (cost == b.cost && position < b.position))
            {
                best = Some(Candidate {
                    position,
                    diagram,
                    cost,
                });
            }
        }
        Ok(best)
    }

    /// Java's `runBlockMoves`: tries moving each run of 2 to 5 adjacent, dependent conditions
    /// to the start, end, and middle of where it may go.
    fn block_moves(&self, state: &mut State) -> Result<(), CompileError> {
        if state.size() > GRANULAR_MAX_NODES {
            return Ok(());
        }
        for block in self.dependency_blocks(&state.order) {
            if !(2..=5).contains(&block.len()) {
                continue;
            }
            let constraints = self.graph.constraints(&state.order);
            if let Some(best) = self.block_move(&block, &state.order, &constraints)? {
                if best.diagram.nodes.len() < state.size() {
                    // Java keeps the moved block's BDD but not its order, so its conditions no
                    // longer match the BDD's variables. Keep both.
                    state.order = best.diagram.conditions.clone();
                    state.best = best.diagram;
                }
            }
        }
        Ok(())
    }

    /// Runs of positions in which each condition depends on the one before it.
    fn dependency_blocks(&self, order: &[usize]) -> Vec<Vec<usize>> {
        let mut blocks = Vec::new();
        let mut current = vec![0];
        for i in 1..order.len() {
            if self.graph.depends_on(order[i], order[i - 1]) {
                current.push(i);
            } else {
                let done = std::mem::replace(&mut current, vec![i]);
                if done.len() >= 2 {
                    blocks.push(done);
                }
            }
        }
        if current.len() >= 2 {
            blocks.push(current);
        }
        blocks
    }

    /// Java's `tryBlockMove`: the smallest, then cheapest, valid move of `block`.
    fn block_move(
        &self,
        block: &[usize],
        order: &[usize],
        constraints: &Constraints,
    ) -> Result<Option<Candidate>, CompileError> {
        let (start, end) = (block[0], block[block.len() - 1]);
        let mut min = 0isize;
        let mut max = order.len() as isize - block.len() as isize;
        for &i in block {
            let offset = (i - start) as isize;
            min = min.max(constraints.min[i] - offset);
            max = max.min(constraints.max[i] - offset);
        }
        if min >= max {
            return Ok(None);
        }
        let mut best: Option<Candidate> = None;
        for target in [min, max, min + (max - min) / 2] {
            if target == start as isize {
                continue;
            }
            // Removing the block and reinserting it at `target` is Java's `moveBlock`.
            let mut candidate = order.to_vec();
            let moved: Vec<usize> = candidate.drain(start..=end).collect();
            let at = target as usize;
            candidate.splice(at..at, moved);
            if !self.graph.constraints(&candidate).is_valid() {
                continue;
            }
            let diagram = self.compile(&candidate)?;
            let cost = self.cost(&diagram);
            let size = diagram.nodes.len();
            if best.as_ref().is_none_or(|b| {
                let best_size = b.diagram.nodes.len();
                size < best_size || (size == best_size && cost < b.cost)
            }) {
                best = Some(Candidate {
                    position: target,
                    diagram,
                    cost,
                });
            }
        }
        Ok(best)
    }

    /// Java's `runAdjacentSwaps`: up to three sweeps swapping neighbors that shrink the BDD.
    fn adjacent_swaps(&self, state: &mut State) -> Result<(), CompileError> {
        if state.size() > GRANULAR_MAX_NODES {
            return Ok(());
        }
        for _ in 0..3 {
            let mut constraints = self.graph.constraints(&state.order);
            let mut improved = false;
            for i in 0..state.order.len() - 1 {
                if !(constraints.can_move(i, i as isize + 1)
                    && constraints.can_move(i + 1, i as isize))
                {
                    continue;
                }
                state.order.swap(i, i + 1);
                let diagram = self.compile(&state.order)?;
                if diagram.nodes.len() < state.size() {
                    state.best = diagram;
                    constraints = self.graph.constraints(&state.order);
                    improved = true;
                } else {
                    state.order.swap(i, i + 1);
                }
            }
            if !improved {
                break;
            }
        }
        Ok(())
    }
}

/// Java's `getStrategicPositions`: every allowed position when there are few, otherwise the
/// ends, evenly spaced positions, and positions near the current one.
fn strategic_positions(
    variable: usize,
    constraints: &Constraints,
    effort: &Effort,
    n: usize,
) -> Vec<usize> {
    let (min, max) = (constraints.min[variable], constraints.max[variable]);
    let range = max - min;
    let allowed = |p: isize| p != variable as isize && constraints.can_move(variable, p);
    // Like Java, these exclude `max` itself.
    if range <= effort.exhaustive_threshold {
        return (min..max)
            .filter(|&p| allowed(p))
            .map(|p| p as usize)
            .collect();
    }
    let limit = effort.max_positions as usize;
    let mut positions = Vec::with_capacity(limit);
    let mut seen = vec![false; n];
    fn add(p: isize, positions: &mut Vec<usize>, seen: &mut [bool]) {
        positions.push(p as usize);
        seen[p as usize] = true;
    }
    if allowed(min) {
        add(min, &mut positions, &mut seen);
    }
    if allowed(max - 1) {
        add(max - 1, &mut positions, &mut seen);
    }
    let step = (range / (effort.max_positions / 2).min(15) as isize).max(1);
    let mut p = min + step;
    while p < max - step && positions.len() < limit {
        if !seen[p as usize] && allowed(p) {
            add(p, &mut positions, &mut seen);
        }
        p += step;
    }
    let radius = effort.nearby_radius as isize;
    for offset in -radius..=radius {
        if positions.len() >= limit {
            break;
        }
        let p = variable as isize + offset;
        if offset != 0 && p >= min && p < max && !seen[p as usize] && allowed(p) {
            add(p, &mut positions, &mut seen);
        }
    }
    positions
}
