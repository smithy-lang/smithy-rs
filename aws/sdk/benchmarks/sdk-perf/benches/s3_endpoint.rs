/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use criterion::{criterion_group, criterion_main, Criterion};
use sdk_perf::s3_endpoint::{
    resolve_s3_accesspoint_endpoint, resolve_s3_outposts_endpoint, resolve_s3_path_style_endpoint,
    resolve_s3_virtual_addressing_endpoint, resolve_s3express_endpoint,
};

fn s3_endpoint_benchmarks(c: &mut Criterion) {
    c.bench_function("s3_outposts_endpoint_resolution", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                resolve_s3_outposts_endpoint();
            }
        })
    });

    c.bench_function("s3_accesspoint_endpoint_resolution", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                resolve_s3_accesspoint_endpoint();
            }
        })
    });

    c.bench_function("s3express_endpoint_resolution", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                resolve_s3express_endpoint();
            }
        })
    });

    c.bench_function("s3_path_style_endpoint_resolution", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                resolve_s3_path_style_endpoint();
            }
        })
    });

    c.bench_function("s3_virtual_addressing_endpoint_resolution", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                resolve_s3_virtual_addressing_endpoint();
            }
        })
    });
}

criterion_group!(benches, s3_endpoint_benchmarks);
criterion_main!(benches);
