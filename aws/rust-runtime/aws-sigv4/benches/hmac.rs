/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sigv4::sign::v4::calculate_signature;
use criterion::{criterion_group, criterion_main, Criterion};

// Benchmarks `calculate_signature` on whichever crypto backend the crate was built with. That is
// one HMAC-SHA256 plus the hex encode of its tag, so the encode and its allocation are in the
// number too; going through the public function rather than the HMAC crate directly is what makes
// it follow the selected backend.
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
