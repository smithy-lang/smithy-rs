/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Java's `CostOptimization`: moves conditions that are expensive and often tested later, and
//! cheap ones earlier, to lower the BDD's expected evaluation cost, while it grows by at most
//! 8%.
//!
//! As in Java, a round makes the single cheapest move among those it tries, then swaps
//! neighbors while that lowers the cost. Ties keep the move tried first.

use super::builder::Diagram;
use super::cfg::Cfg;
use super::cost::{condition_cost, expected_cost, expected_evaluations};
use super::order::{Constraints, DependencyGraph};
use super::search::{move_to, with_compiler, Compiler};
use super::CompileError;

/// How much the BDD may grow, as a fraction of its size.
const MAX_ALLOWED_GROWTH: f64 = 0.08;
const MAX_ROUNDS: usize = 30;
/// How many of the hottest positions a round tries to move.
const TOP_K: usize = 50;

/// The order being optimized, its BDD, and that BDD's expected cost.
struct State {
    order: Vec<usize>,
    bdd: Diagram,
    cost: f64,
    constraints: Constraints,
}

struct Optimizer<'a> {
    compiler: &'a Compiler<'a>,
    graph: DependencyGraph,
    /// The cost of each canonical condition.
    costs: Vec<i32>,
    max_nodes: usize,
}

/// Lowers `diagram`'s expected evaluation cost.
pub(super) fn optimize(cfg: &Cfg, diagram: Diagram) -> Result<Diagram, CompileError> {
    let baseline = diagram.nodes.len();
    with_compiler(cfg, diagram.conditions.len(), |compiler| {
        let optimizer = Optimizer {
            compiler,
            graph: DependencyGraph::new(cfg),
            costs: cfg.canonical.iter().map(condition_cost).collect(),
            // Java truncates the allowance to an `int`.
            max_nodes: baseline + (baseline as f64 * MAX_ALLOWED_GROWTH) as usize,
        };
        optimizer.run(diagram)
    })
}

impl Optimizer<'_> {
    fn run(&self, diagram: Diagram) -> Result<Diagram, CompileError> {
        let mut state = State {
            order: diagram.conditions.clone(),
            cost: self.cost(&diagram),
            constraints: self.graph.constraints(&diagram.conditions),
            bdd: diagram,
        };
        for _ in 0..MAX_ROUNDS {
            let Some((from, to)) = self.best_move(&state)? else {
                break;
            };
            move_to(&mut state.order, from, to);
            state.bdd = self.compiler.compile(&state.order)?;
            state.cost = self.cost(&state.bdd);
            state.constraints = self.graph.constraints(&state.order);
            self.polish(&mut state)?;
        }
        Ok(state.bdd)
    }

    fn cost(&self, diagram: &Diagram) -> f64 {
        expected_cost(diagram, &self.order_costs(&diagram.conditions))
    }

    fn order_costs(&self, order: &[usize]) -> Vec<i32> {
        order.iter().map(|&c| self.costs[c]).collect()
    }

    /// Java's `findBestMove`: the cheapest move, if cheaper than the current order, of the
    /// hottest positions to targets near them and at the ends of where they may go.
    fn best_move(&self, state: &State) -> Result<Option<(usize, usize)>, CompileError> {
        let n = state.order.len();
        let mut moves = Vec::new();
        for hot in self.hot_positions(state) {
            for target in targets(hot, &state.constraints, n) {
                if state.constraints.can_move(hot, target as isize) {
                    moves.push((hot, target));
                }
            }
        }
        let mut best = None;
        let mut best_cost = state.cost;
        for (i, diagram) in self
            .compiler
            .compile_moves(&state.order, &moves, self.max_nodes)?
        {
            let cost = self.cost(&diagram);
            if cost < best_cost {
                best_cost = cost;
                best = Some(moves[i]);
            }
        }
        Ok(best)
    }

    /// Java's `getHotPositions`: the positions with the most expected cost, highest first.
    fn hot_positions(&self, state: &State) -> Vec<usize> {
        let costs = self.order_costs(&state.order);
        let reach = expected_evaluations(&state.bdd, costs.len());
        let hotness: Vec<f64> = reach
            .iter()
            .zip(&costs)
            .map(|(&r, &c)| r * f64::from(c))
            .collect();
        let mut positions: Vec<usize> = (0..hotness.len()).collect();
        // `total_cmp` orders like Java's `Double.compare`, and the sort is stable like Java's.
        positions.sort_by(|&a, &b| hotness[b].total_cmp(&hotness[a]));
        positions.truncate(TOP_K);
        positions
    }

    /// Java's `polish`: up to two sweeps swapping neighbors where that lowers the cost.
    fn polish(&self, state: &mut State) -> Result<(), CompileError> {
        for _ in 0..2 {
            let mut improved = false;
            for i in 0..state.order.len().saturating_sub(1) {
                if !state.constraints.can_move(i + 1, i as isize)
                    || !state.constraints.can_move(i, i as isize + 1)
                {
                    continue;
                }
                let mut candidate = state.order.clone();
                candidate.swap(i, i + 1);
                let bdd = self.compiler.compile(&candidate)?;
                if bdd.nodes.len() > self.max_nodes {
                    continue;
                }
                let cost = self.cost(&bdd);
                if cost < state.cost {
                    state.constraints = self.graph.constraints(&candidate);
                    state.order = candidate;
                    state.bdd = bdd;
                    state.cost = cost;
                    improved = true;
                }
            }
            if !improved {
                break;
            }
        }
        Ok(())
    }
}

/// Java's `buildTargets`: the earliest allowed position, the start, positions 1 to 32 earlier,
/// the latest allowed position, and positions 1 to 4 later, without repeats.
fn targets(hot: usize, constraints: &Constraints, n: usize) -> Vec<usize> {
    let hot_at = hot as isize;
    let candidates = [constraints.min[hot], 0]
        .into_iter()
        .chain([1, 2, 4, 8, 16, 32].map(|d| hot_at - d))
        .chain([constraints.max[hot]])
        .chain([1, 2, 4].map(|d| hot_at + d));
    let mut seen = vec![false; n];
    let mut targets = Vec::new();
    for target in candidates {
        if target >= 0 && target < n as isize && target != hot_at && !seen[target as usize] {
            seen[target as usize] = true;
            targets.push(target as usize);
        }
    }
    targets
}
