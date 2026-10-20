/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Compiles the candidate variable orders the optimizers try, in parallel when a search has
//! more than [`PARALLEL_THRESHOLD`] of them.
//!
//! Results come back in the order the candidates were given, and the optimizers choose among
//! them in that order, so the BDDs they produce do not depend on threads.

use super::builder::{self, BddBuilder, Diagram};
use super::cfg::Cfg;
use super::CompileError;
use std::cell::RefCell;
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc;
use std::thread::{self, Scope};

/// Java's `SiftingOptimization.PARALLEL_THRESHOLD`: searches with more candidates than this
/// are compiled in parallel.
const PARALLEL_THRESHOLD: usize = 7;

/// Moves the condition at `from` to `to`, shifting those between (Java's
/// `BddCompilerSupport.move`).
pub(super) fn move_to(order: &mut Vec<usize>, from: usize, to: usize) {
    let condition = order.remove(from);
    order.insert(to, condition);
}

/// Compiles candidate orders for one optimizer.
pub(super) struct Compiler<'a> {
    cfg: &'a Cfg,
    builder: RefCell<BddBuilder>,
    workers: Option<Workers>,
}

/// Runs `optimize` with a [`Compiler`] for `cfg`, whose `conditions` the optimizer orders.
pub(super) fn with_compiler<R>(
    cfg: &Cfg,
    conditions: usize,
    optimize: impl FnOnce(&Compiler<'_>) -> R,
) -> R {
    // Only a search with more candidates than conditions could be parallel.
    let threads = if conditions > PARALLEL_THRESHOLD {
        thread::available_parallelism().map_or(1, |n| n.get())
    } else {
        1
    };
    thread::scope(|scope| {
        let compiler = Compiler {
            cfg,
            builder: RefCell::new(BddBuilder::new()),
            workers: (threads > 1).then(|| Workers::new(scope, cfg, threads)),
        };
        // Dropping the compiler, after this, stops its workers.
        optimize(&compiler)
    })
}

impl Compiler<'_> {
    pub(super) fn compile(&self, order: &[usize]) -> Result<Diagram, CompileError> {
        builder::compile_with(&mut self.builder.borrow_mut(), self.cfg, order.to_vec())
    }

    /// The BDDs of `order` with each of `moves` (from, to) applied that have at most `limit`
    /// nodes, with the index of their move, in order of move.
    pub(super) fn compile_moves(
        &self,
        order: &[usize],
        moves: &[(usize, usize)],
        limit: usize,
    ) -> Result<Moves, CompileError> {
        let job = |first: usize, moves: &[(usize, usize)]| Job {
            order: order.to_vec(),
            first,
            moves: moves.to_vec(),
            limit,
        };
        let workers = match &self.workers {
            Some(workers) if moves.len() > PARALLEL_THRESHOLD => workers,
            _ => return compile_moves(&mut self.builder.borrow_mut(), self.cfg, &job(0, moves)),
        };
        let size = moves.len().div_ceil(workers.jobs.len());
        let chunks: Vec<_> = moves.chunks(size).collect();
        for (i, (chunk, sender)) in chunks.iter().zip(&workers.jobs).enumerate() {
            sender
                .send(job(i * size, chunk))
                .expect("workers outlive the compiler");
        }
        let mut kept = Vec::new();
        let mut failed = None;
        // Collect every reply, so no worker is left with a stale one.
        for receiver in &workers.replies[..chunks.len()] {
            match receiver.recv().expect("workers outlive the compiler") {
                Ok(Ok(moves)) => kept.extend(moves),
                Ok(Err(error)) => failed = failed.or(Some(Ok(error))),
                Err(panic) => failed = Some(Err(panic)),
            }
        }
        match failed {
            None => Ok(kept),
            Some(Ok(error)) => Err(error),
            Some(Err(panic)) => panic::resume_unwind(panic),
        }
    }
}

/// Compiled moves, each with its index.
pub(super) type Moves = Vec<(usize, Diagram)>;

/// A request to compile `order` with each of `moves` applied, numbering them from `first`
/// and keeping BDDs of at most `limit` nodes.
struct Job {
    order: Vec<usize>,
    first: usize,
    moves: Vec<(usize, usize)>,
    limit: usize,
}

type Reply = thread::Result<Result<Moves, CompileError>>;

/// Threads that compile candidates, each with its own builder. They live as long as the
/// optimizer, because threads started per search spend more time warming up than compiling.
struct Workers {
    jobs: Vec<mpsc::Sender<Job>>,
    replies: Vec<mpsc::Receiver<Reply>>,
}

impl Workers {
    fn new<'scope>(scope: &'scope Scope<'scope, '_>, cfg: &'scope Cfg, threads: usize) -> Self {
        let (mut jobs, mut replies) = (Vec::new(), Vec::new());
        for _ in 0..threads {
            let (job_sender, job_receiver) = mpsc::channel::<Job>();
            let (reply_sender, reply_receiver) = mpsc::channel();
            scope.spawn(move || {
                let mut builder = BddBuilder::new();
                // Ends when the compiler, which holds the job sender, is dropped.
                for job in job_receiver {
                    let reply = panic::catch_unwind(AssertUnwindSafe(|| {
                        compile_moves(&mut builder, cfg, &job)
                    }));
                    if reply_sender.send(reply).is_err() {
                        break;
                    }
                }
            });
            jobs.push(job_sender);
            replies.push(reply_receiver);
        }
        Self { jobs, replies }
    }
}

fn compile_moves(builder: &mut BddBuilder, cfg: &Cfg, job: &Job) -> Result<Moves, CompileError> {
    let mut kept = Vec::new();
    for (i, &(from, to)) in job.moves.iter().enumerate() {
        let mut order = job.order.clone();
        move_to(&mut order, from, to);
        let diagram = builder::compile_with(builder, cfg, order)?;
        if diagram.nodes.len() <= job.limit {
            kept.push((job.first + i, diagram));
        }
    }
    Ok(kept)
}
