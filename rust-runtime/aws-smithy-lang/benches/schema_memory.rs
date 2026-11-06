/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Allocation and retained-memory report for `RuntimeSchemaSet`, for SQS, S3, and EC2.
//!
//! Separate from `benches/model.rs` because it installs a counting global allocator, which
//! would make those timings incomparable with the recorded baselines. Prints a table rather
//! than timing anything:
//!
//! ```text
//! cargo bench -p aws-smithy-lang --features runtime-schema --bench schema_memory
//! ```
//!
//! See `benches/README.md` for the recorded results.

use aws_smithy_lang::runtime_schema::{Materialization, OperationSchema, RuntimeSchemaSet};
use aws_smithy_lang::traversal::Walker;
use aws_smithy_lang::{Model, ShapeId};
use std::alloc::{GlobalAlloc, Layout, System};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwards the caller's layout unchanged to the system allocator.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            ALLOCATIONS.fetch_add(1, Relaxed);
            ALLOCATED.fetch_add(layout.size(), Relaxed);
            let live = LIVE.fetch_add(layout.size(), Relaxed) + layout.size();
            PEAK.fetch_max(live, Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` was allocated by `alloc` above with this layout.
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Relaxed);
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const MODELS: [&str; 3] = ["sqs", "s3", "ec2"];

fn read(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../aws/sdk/aws-models")
        .join(format!("{name}.json"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

/// What one step allocated, and what it left allocated.
struct Usage {
    allocations: usize,
    allocated: usize,
    /// Peak live bytes during the step, above the live bytes before it.
    peak: usize,
    retained: isize,
}

/// Runs `f`, keeping its result alive until the usage is recorded.
fn measure<T>(f: impl FnOnce() -> T) -> (T, Usage) {
    let (allocations, allocated, live) = (
        ALLOCATIONS.load(Relaxed),
        ALLOCATED.load(Relaxed),
        LIVE.load(Relaxed),
    );
    PEAK.store(live, Relaxed);
    let value = f();
    let usage = Usage {
        allocations: ALLOCATIONS.load(Relaxed) - allocations,
        allocated: ALLOCATED.load(Relaxed) - allocated,
        peak: PEAK.load(Relaxed) - live,
        retained: LIVE.load(Relaxed) as isize - live as isize,
    };
    (value, usage)
}

/// The number of schemas a set has built, from its `Debug` output.
fn built(set: &RuntimeSchemaSet) -> usize {
    let debug = format!("{set:?}");
    let count = debug.split("built_schemas: ").nth(1).unwrap();
    count.split(',').next().unwrap().parse().unwrap()
}

fn kib(bytes: impl TryInto<i64>) -> String {
    let bytes = bytes.try_into().ok().unwrap() as f64;
    if bytes.abs() >= 1024.0 * 1024.0 {
        format!("{:.2} MiB", bytes / (1024.0 * 1024.0))
    } else {
        format!("{:.1} KiB", bytes / 1024.0)
    }
}

fn row(model: &str, case: &str, schemas: Option<usize>, usage: &Usage) {
    println!(
        "| {model} | {case} | {} | {} | {} | {} | {} |",
        schemas.map_or("–".into(), |n| n.to_string()),
        usage.allocations,
        kib(usage.allocated as i64),
        kib(usage.peak as i64),
        kib(usage.retained as i64),
    );
}

fn lazy(model: &Model, service: &ShapeId) -> RuntimeSchemaSet {
    RuntimeSchemaSet::builder(model)
        .for_service(service)
        .materialization(Materialization::Lazy)
        .build()
        .unwrap()
}

fn main() {
    // `cargo bench` passes `--bench`; there is nothing to filter.
    println!(
        "`size_of::<OperationSchema>()` = {} bytes\n",
        std::mem::size_of::<OperationSchema<'static>>()
    );
    println!("| Model | Case | Schemas | Allocations | Allocated | Peak | Retained |");
    println!("| ----- | ---- | ------: | ----------: | --------: | ---: | -------: |");
    for name in MODELS {
        let bytes = read(name);
        let (model, usage) = measure(|| Model::from_json_slice(name, &bytes).unwrap());
        row(name, "model", None, &usage);
        drop(bytes);
        let service = model.services().next().unwrap().id().clone();

        let (set, usage) = measure(|| RuntimeSchemaSet::try_from(&model).unwrap());
        row(name, "eager all shapes", Some(built(&set)), &usage);
        drop(set);

        let (eager, usage) = measure(|| {
            RuntimeSchemaSet::builder(&model)
                .for_service(&service)
                .build()
                .unwrap()
        });
        let eager_schemas = built(&eager);
        row(name, "eager service", Some(eager_schemas), &usage);
        let ids: Vec<ShapeId> = eager.operation_ids().cloned().collect();
        drop(eager);

        let (set, usage) = measure(|| lazy(&model, &service));
        row(
            name,
            "lazy service, nothing built",
            Some(built(&set)),
            &usage,
        );
        drop(set);

        let (set, usage) = measure(|| {
            RuntimeSchemaSet::builder(&model)
                .for_service_operations(&service, &ids[..1])
                .materialization(Materialization::Lazy)
                .build()
                .unwrap()
        });
        row(
            name,
            "lazy, 1 operation selected",
            Some(built(&set)),
            &usage,
        );
        drop(set);

        // Operations ranked by the shapes they reach, as in `schema/first_operation`.
        let mut ranked: Vec<_> = model
            .operations()
            .map(|operation| {
                (
                    Walker::new().walk(operation).count(),
                    operation.id().clone(),
                )
            })
            .collect();
        ranked.sort();
        let at = |fraction: f64| &ranked[((ranked.len() - 1) as f64 * fraction) as usize].1;
        for (rank, operation) in [("p50", at(0.5)), ("p90", at(0.9)), ("max", at(1.0))] {
            let set = lazy(&model, &service);
            let (_, usage) = measure(|| {
                set.operation(operation).unwrap();
            });
            row(
                name,
                &format!("first operation, {rank}"),
                Some(built(&set)),
                &usage,
            );
        }

        // Ten consecutive operations in service order (which often share shapes), and ten
        // spread evenly through the service.
        let count = ids.len().min(10);
        let stride = ids.len() / count;
        for (case, selected) in [
            ("10 consecutive operations", ids[..count].to_vec()),
            (
                "10 spread operations",
                ids.iter().step_by(stride).take(count).cloned().collect(),
            ),
        ] {
            let set = lazy(&model, &service);
            let (_, usage) = measure(|| {
                for id in &selected {
                    set.operation(id).unwrap();
                }
            });
            row(name, case, Some(built(&set)), &usage);
        }

        let (set, usage) = measure(|| {
            let set = lazy(&model, &service);
            set.materialize_all();
            set
        });
        let all = built(&set);
        assert_eq!(all, eager_schemas, "{name}");
        row(name, "lazy service, then everything", Some(all), &usage);
        drop(set);

        // Build, use, and drop repeatedly: nothing is left behind except interned HTTP
        // header names, which are shared by every set and built only once.
        let (_, usage) = measure(|| {
            for _ in 0..10 {
                let set = lazy(&model, &service);
                set.materialize_all();
            }
        });
        row(name, "10 lazy build/use/drop cycles", None, &usage);
    }
}
