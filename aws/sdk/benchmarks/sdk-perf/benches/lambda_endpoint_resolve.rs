/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use sdk_perf::lambda_endpoint::LambdaEndpointBenchmark;
use tokio::runtime::Runtime;

fn lambda_endpoint_benchmarks(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let benches = [
        ("lambda_standard", LambdaEndpointBenchmark::standard()),
        (
            "lambda_govcloud_fips_dualstack",
            LambdaEndpointBenchmark::govcloud_fips_dualstack(),
        ),
    ];

    for (name, bench) in &benches {
        c.bench_function(name, |b| {
            b.to_async(&rt)
                .iter(|| async { black_box(bench.resolve().await) });
        });
    }
}

criterion_group!(benches, lambda_endpoint_benchmarks);
criterion_main!(benches);
