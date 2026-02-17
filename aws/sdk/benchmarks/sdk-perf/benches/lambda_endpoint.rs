/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use criterion::{criterion_group, criterion_main, Criterion};
use sdk_perf::lambda_endpoint::{
    resolve_lambda_govcloud_fips_dualstack_endpoint, resolve_lambda_standard_endpoint,
};

fn lambda_endpoint_benchmarks(c: &mut Criterion) {
    c.bench_function("lambda_standard_endpoint_resolution", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                resolve_lambda_standard_endpoint();
            }
        })
    });

    c.bench_function("lambda_govcloud_fips_dualstack_endpoint_resolution", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                resolve_lambda_govcloud_fips_dualstack_endpoint();
            }
        })
    });
}

criterion_group!(benches, lambda_endpoint_benchmarks);
criterion_main!(benches);
