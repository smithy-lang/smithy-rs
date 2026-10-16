/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sigv4::sign::v4::calculate_signature;
use criterion::{criterion_group, criterion_main, Criterion};

// Benchmarks the HMAC-SHA256 that SigV4 signature calculation is made of, on whichever crypto
// backend the crate was built with. It goes through `calculate_signature` rather than the HMAC
// crate directly so that the numbers follow the selected backend.
pub fn hmac(c: &mut Criterion) {
    c.bench_function("hmac", |b| {
        b.iter(|| calculate_signature(b"secret", b"hello, world"))
    });
}

criterion_group! {
    name = benches;

    config = Criterion::default();

    targets = hmac
}

criterion_main!(benches);
