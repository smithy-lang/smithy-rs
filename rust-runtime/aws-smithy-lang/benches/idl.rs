/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Smithy IDL loading benchmarks, compared with the equivalent JSON AST in the same binary.
//! See `benches/README.md` for methodology and the recorded baseline.

#[path = "../tests/support/to_idl.rs"]
mod to_idl;

use aws_smithy_lang::Model;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::alloc::{GlobalAlloc, Layout as AllocLayout, System};
use std::hint::black_box;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use to_idl::{assemble, render, Layout, Part};

/// Counts allocations so the memory report can compare representations. Both formats pay the
/// same counting overhead in the timed benchmarks.
struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: AllocLayout) -> *mut u8 {
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

    unsafe fn dealloc(&self, ptr: *mut u8, layout: AllocLayout) {
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

struct Input {
    name: &'static str,
    json: Vec<u8>,
    /// One IDL file per namespace.
    idl: Vec<Part>,
    /// The shapes spread over 64 IDL files per namespace.
    files: Vec<Part>,
    /// Alternate shapes in IDL and JSON AST.
    mixed: Vec<Part>,
}

impl Input {
    fn idl_bytes(&self) -> u64 {
        self.idl.iter().map(|p| p.text().len() as u64).sum()
    }
}

fn inputs() -> Vec<Input> {
    MODELS
        .iter()
        .map(|name| {
            let json = read(name);
            let model = Model::from_json_slice(name, &json).unwrap();
            let input = Input {
                name,
                idl: render(&model, Layout::PerNamespace),
                files: render(&model, Layout::Files(64)),
                mixed: render(&model, Layout::Mixed),
                json,
            };
            // Benchmarks are only meaningful if every rendering is the same model.
            for parts in [&input.idl, &input.files, &input.mixed] {
                assert!(assemble(parts).unwrap().equivalent(&model), "{name}");
            }
            input
        })
        .collect()
}

/// Allocations, allocated bytes, peak live bytes during `f`, and bytes still live after it
/// (the retained model).
fn measure<T>(f: impl FnOnce() -> T) -> (usize, usize, usize, usize, T) {
    let base = LIVE.load(Relaxed);
    let allocations = ALLOCATIONS.load(Relaxed);
    let allocated = ALLOCATED.load(Relaxed);
    PEAK.store(base, Relaxed);
    let value = f();
    (
        ALLOCATIONS.load(Relaxed) - allocations,
        ALLOCATED.load(Relaxed) - allocated,
        PEAK.load(Relaxed) - base,
        LIVE.load(Relaxed).saturating_sub(base),
        value,
    )
}

fn memory_report(inputs: &[Input]) {
    eprintln!("| Model | Input | Bytes | Allocations | Allocated | Peak live | Retained |");
    eprintln!("| ----- | ----- | ----: | ----------: | --------: | --------: | -------: |");
    let mib = |bytes: usize| format!("{:.1} MiB", bytes as f64 / (1 << 20) as f64);
    for input in inputs {
        let cases: [(&str, u64, &dyn Fn() -> Model); 2] = [
            ("JSON", input.json.len() as u64, &|| {
                Model::from_json_slice(input.name, &input.json).unwrap()
            }),
            ("IDL", input.idl_bytes(), &|| assemble(&input.idl).unwrap()),
        ];
        for (label, bytes, load) in cases {
            let (allocations, allocated, peak, retained, model) = measure(load);
            drop(model);
            eprintln!(
                "| {} | {label} | {} | {allocations} | {} | {} | {} |",
                input.name,
                mib(bytes as usize),
                mib(allocated),
                mib(peak),
                mib(retained),
            );
        }
    }
}

/// A node- and text-block-heavy document: large traits, metadata, and documentation.
fn strings_document() -> String {
    let mut text = String::from("$version: \"2\"\nmetadata config = {\n");
    for i in 0..2_000 {
        text.push_str(&format!(
            "    key{i}: {{ list: [1, 2.5, true, null, \"s\\n{i}\"], nested: {{ a: {{ b: [\"c\"] }} }} }}\n"
        ));
    }
    text.push_str("}\nnamespace ex\n");
    for i in 0..2_000 {
        // Alternate documentation comments and text-block documentation traits.
        if i % 2 == 0 {
            text.push_str(&format!(
                "/// Shape {i}\n/// with documentation\n///   indented\n"
            ));
        } else {
            text.push_str(&format!(
                "@documentation(\"\"\"\n    Text block {i}\n        indented \\\"\"\" line\n    end\n    \"\"\")\n"
            ));
        }
        text.push_str(&format!("@tags([\"a\", \"b\"])\nstring S{i}\n"));
    }
    text
}

/// `text` with a syntax error inserted at the line break nearest `fraction` of its length.
fn corrupt(text: &str, fraction: f64) -> String {
    let at = (text.len() as f64 * fraction) as usize;
    let at = text[..at].rfind('\n').map_or(0, |i| i + 1);
    format!("{}structure {{\n{}", &text[..at], &text[at..])
}

fn bench(c: &mut Criterion) {
    let inputs = inputs();
    memory_report(&inputs);

    let mut group = c.benchmark_group("parse");
    for input in &inputs {
        group.throughput(Throughput::Bytes(input.json.len() as u64));
        group.bench_with_input(BenchmarkId::new("json", input.name), input, |b, input| {
            b.iter(|| Model::from_json_slice(input.name, black_box(&input.json)).unwrap())
        });
        group.throughput(Throughput::Bytes(input.idl_bytes()));
        group.bench_with_input(BenchmarkId::new("idl", input.name), input, |b, input| {
            b.iter(|| assemble(black_box(&input.idl)).unwrap())
        });
    }
    group.finish();

    let mut group = c.benchmark_group("assemble");
    for input in &inputs {
        group.bench_with_input(
            BenchmarkId::new("idl_64_files", input.name),
            input,
            |b, input| b.iter(|| assemble(black_box(&input.files)).unwrap()),
        );
        group.bench_with_input(BenchmarkId::new("mixed", input.name), input, |b, input| {
            b.iter(|| assemble(black_box(&input.mixed)).unwrap())
        });
    }
    group.finish();

    let strings = strings_document();
    let mut group = c.benchmark_group("strings");
    group.throughput(Throughput::Bytes(strings.len() as u64));
    group.bench_function("idl", |b| {
        b.iter(|| Model::from_idl_str("strings.smithy", black_box(&strings)).unwrap())
    });
    group.finish();

    let s3 = inputs.iter().find(|i| i.name == "s3").unwrap();
    let text = s3
        .idl
        .iter()
        .map(Part::text)
        .max_by_key(|t| t.len())
        .unwrap();
    let mut group = c.benchmark_group("syntax_error");
    for (label, fraction) in [("early", 0.01), ("late", 0.99)] {
        let broken = corrupt(text, fraction);
        assert!(Model::from_idl_str("s3.smithy", &broken)
            .unwrap_err()
            .is_idl_syntax());
        group.bench_function(label, |b| {
            b.iter(|| Model::from_idl_str("s3.smithy", black_box(&broken)).unwrap_err())
        });
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
