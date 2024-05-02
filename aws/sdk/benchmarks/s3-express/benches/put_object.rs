/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::BehaviorVersion;
use aws_sdk_s3::primitives::ByteStream;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use s3_express::{confidence_level, number_of_iterations, sample_size};
use tokio::runtime::Runtime;

pub fn put_object(c: &mut Criterion) {
    let buckets = if let Ok(buckets) = std::env::var("BUCKETS") {
        buckets.split(",").map(String::from).collect::<Vec<_>>()
    } else {
        panic!("required environment variable `BUCKETS` should be set: e.g. `BUCKETS=\"bucket1,bucket2\"`")
    };

    let number_of_iterations = number_of_iterations();

    println!(
        "measuring {number_of_iterations} of PutObject against {buckets:#?}, \
        switching buckets on every operation if more than one bucket is specified\n"
    );

    let client = Runtime::new().unwrap().block_on(async {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        aws_sdk_s3::Client::new(&config)
    });

    let mut group = c.benchmark_group("put_object");

    let key = "test";
    const KB: usize = 1024;
    for size in [64 * KB, KB * KB].iter() {
        let object: Vec<u8> = vec![0; *size];
        group.bench_with_input(BenchmarkId::new("size", size), size, |b, _| {
            b.to_async(Runtime::new().unwrap()).iter(|| async {
                for i in 0..number_of_iterations {
                    let bucket = &buckets[i % buckets.len()];
                    client
                        .put_object()
                        .bucket(bucket)
                        .key(key)
                        .body(ByteStream::from(object.clone()))
                        .send()
                        .await
                        .unwrap();
                }
            });
        });
    }
    group.finish();

    // Clean up test objects
    Runtime::new().unwrap().block_on(async {
        for bucket in buckets {
            client
                .delete_object()
                .bucket(bucket)
                .key(key)
                .send()
                .await
                .unwrap();
        }
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(sample_size()).confidence_level(confidence_level());
    targets = put_object
);
criterion_main!(benches);
