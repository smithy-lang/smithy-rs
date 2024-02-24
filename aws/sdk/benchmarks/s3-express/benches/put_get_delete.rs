/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::BehaviorVersion;
use aws_sdk_s3::primitives::ByteStream;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use tokio::runtime::Runtime;

const DEFAULT_CONFIDENCE_LEVEL: f64 = 0.99;
const DEFAULT_NUMBER_OF_ITERATIONS: usize = 100;
const DEFAULT_SAMPLE_SIZE: usize = 10;

fn sample_size() -> usize {
    let sample_size =
        std::env::var("SAMPLE_SIZE").map_or(DEFAULT_SAMPLE_SIZE, |s| s.parse::<usize>().unwrap());
    dbg!(sample_size)
}

fn confidence_level() -> f64 {
    let confidence_level = std::env::var("CONFIDENCE_LEVEL")
        .map_or(DEFAULT_CONFIDENCE_LEVEL, |s| s.parse::<f64>().unwrap());
    dbg!(confidence_level)
}

pub fn put_get_delete(c: &mut Criterion) {
    let buckets = if let Ok(buckets) = std::env::var("BUCKETS") {
        buckets.split(",").map(String::from).collect::<Vec<_>>()
    } else {
        panic!("required environment variable `BUCKETS` should be set: e.g. `BUCKETS=\"bucket1,bucket2\"`")
    };

    let number_of_iterations = std::env::var("NUMBER_OF_ITERATIONS")
        .map_or(DEFAULT_NUMBER_OF_ITERATIONS, |n| {
            n.parse::<usize>().unwrap()
        });

    println!(
        "measuring {number_of_iterations} sequences of \
            PutObject -> GetObject -> DeleteObject against \
            {buckets:#?}, switching buckets on every sequence of operations\n"
    );

    let client = Runtime::new().unwrap().block_on(async {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        aws_sdk_s3::Client::new(&config)
    });

    let mut group = c.benchmark_group("put_get_delete");
    group.sample_size(10).confidence_level(0.99);

    const KB: usize = 1024;
    for size in [64 * KB, KB * KB].iter() {
        let object: Vec<u8> = vec![0; *size];
        group.bench_with_input(BenchmarkId::new("size", size), size, |b, _| {
            b.to_async(Runtime::new().unwrap()).iter(|| async {
                for i in 0..number_of_iterations {
                    let bucket = &buckets[i % buckets.len()];
                    let key = "test";
                    client
                        .put_object()
                        .bucket(bucket)
                        .key(key)
                        .body(ByteStream::from(object.clone()))
                        .send()
                        .await
                        .unwrap();

                    client
                        .get_object()
                        .bucket(bucket)
                        .key(key)
                        .send()
                        .await
                        .unwrap();

                    client
                        .delete_object()
                        .bucket(bucket)
                        .key(key)
                        .send()
                        .await
                        .unwrap();
                }
            });
        });
    }
    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(sample_size()).confidence_level(confidence_level());
    targets = put_get_delete
);
criterion_main!(benches);
