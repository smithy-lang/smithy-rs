/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use criterion::{criterion_group, criterion_main, Criterion};
#[cfg(not(feature = "__aws-lc-rs"))]
use hmac::digest::FixedOutput;
#[cfg(not(feature = "__aws-lc-rs"))]
use hmac::{Hmac, KeyInit, Mac};
#[cfg(not(feature = "__aws-lc-rs"))]
use sha2::Sha256;

#[cfg(not(feature = "__aws-lc-rs"))]
pub fn hmac(c: &mut Criterion) {
    c.bench_function("hmac", |b| {
        b.iter(|| {
            let mut mac = Hmac::<Sha256>::new_from_slice(b"secret").unwrap();

            mac.update(b"hello, world");
            mac.finalize_fixed()
        })
    });
}

#[cfg(feature = "__aws-lc-rs")]
pub fn hmac(c: &mut Criterion) {
    c.bench_function("hmac", |b| {
        b.iter(|| {
            let key = aws_lc_rs::hmac::Key::new(aws_lc_rs::hmac::HMAC_SHA256, b"secret");
            aws_lc_rs::hmac::sign(&key, b"hello, world")
        })
    });
}

criterion_group! {
    name = benches;

    config = Criterion::default();

    targets = hmac
}

criterion_main!(benches);
