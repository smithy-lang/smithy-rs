/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Parse, write, and service-closure traversal benchmarks for SQS, S3, and EC2.
//! See `benches/README.md` for methodology and the recorded baseline.

use aws_smithy_lang::traversal::Walker;
use aws_smithy_lang::{Model, ModelWriter};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::path::PathBuf;

const MODELS: [&str; 3] = ["sqs", "s3", "ec2"];

fn read(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../aws/sdk/aws-models")
        .join(format!("{name}.json"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

fn bench(c: &mut Criterion) {
    let inputs: Vec<_> = MODELS.iter().map(|name| (*name, read(name))).collect();

    let mut group = c.benchmark_group("parse");
    for (name, bytes) in &inputs {
        group.throughput(Throughput::Bytes(bytes.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), bytes, |b, bytes| {
            b.iter(|| Model::from_json_slice(name, black_box(bytes)).unwrap())
        });
    }
    group.finish();

    let models: Vec<_> = inputs
        .iter()
        .map(|(name, bytes)| (*name, Model::from_json_slice(name, bytes).unwrap()))
        .collect();

    let mut group = c.benchmark_group("write");
    let writer = ModelWriter::new();
    for (name, model) in &models {
        group.bench_with_input(BenchmarkId::from_parameter(name), model, |b, model| {
            let mut out = Vec::with_capacity(8 << 20);
            b.iter(|| {
                out.clear();
                writer.write(black_box(model), &mut out).unwrap();
                out.len()
            })
        });
    }
    group.finish();

    let mut group = c.benchmark_group("service_closure");
    for (name, model) in &models {
        let service = model.services().next().expect("one service per model");
        group.bench_with_input(BenchmarkId::from_parameter(name), model, |b, model| {
            b.iter(|| Walker::new(model).walk(*black_box(service)).count())
        });
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
