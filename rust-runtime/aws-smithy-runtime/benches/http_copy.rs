/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn remake(parts: http::request::Parts) -> http::Request<()> {
    let mut new_request = http::Request::builder();
    new_request = new_request.uri(parts.uri.into_parts());
    let mut prev_key = None;
    for (key, v) in parts.headers.into_iter() {
        if let Some(key) = key {
            prev_key = Some(key);
        }

        new_request = new_request.header(prev_key.as_ref().unwrap(), v.as_bytes());
    }
    new_request.body(()).unwrap()
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("fib 20", |b| {
        b.iter(|| {
            let initial_request = http::Request::builder()
                .header("a", "b")
                .header("k", "d")
                .header("e", "f")
                .header("x-amz-auth", "long header value")
                .header("a", "b")
                .header("k", "d")
                .header("e", "f")
                .header("x-amz-auth", "long header value")
                .uri("http://www.google.com")
                .body(())
                .unwrap();
            let (parts, _) = initial_request.into_parts();
            //remake(black_box(parts))
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
