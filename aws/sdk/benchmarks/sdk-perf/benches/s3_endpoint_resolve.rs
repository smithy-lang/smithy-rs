/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use sdk_perf::s3_endpoint::S3EndpointBenchmark;
use tokio::runtime::Runtime;

fn s3_endpoint_benchmarks(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let benches = [
        ("s3_outposts", S3EndpointBenchmark::s3_outposts()),
        ("s3_accesspoint", S3EndpointBenchmark::s3_accesspoint()),
        ("s3express", S3EndpointBenchmark::s3express()),
        ("s3_path_style", S3EndpointBenchmark::s3_path_style()),
        (
            "s3_virtual_addressing",
            S3EndpointBenchmark::s3_virtual_addressing(),
        ),
    ];

    for (name, bench) in &benches {
        c.bench_function(name, |b| {
            b.to_async(&rt)
                .iter(|| async { black_box(bench.resolve().await) });
        });
    }
}

criterion_group!(benches, s3_endpoint_benchmarks);
criterion_main!(benches);
