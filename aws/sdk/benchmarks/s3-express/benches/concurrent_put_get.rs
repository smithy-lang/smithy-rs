/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::BehaviorVersion;
use aws_sdk_s3::primitives::ByteStream;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use tokio::runtime::Runtime;
use tokio::task;

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

pub fn concurrent_put_get(c: &mut Criterion) {
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
        "measuring {number_of_iterations} concurrent PutObject followed by \
            {number_of_iterations} concurrent GetObject against \
            {buckets:#?}, with each bucket being assigned equal number of operations\n"
    );

    let client = Runtime::new().unwrap().block_on(async {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        aws_sdk_s3::Client::new(&config)
    });

    const KB: usize = 1024;
    const SIZE: usize = 64 * KB;
    let object: Vec<u8> = vec![0; SIZE];
    let mut group = c.benchmark_group("concurrent_put_delete");
    group.bench_with_input(BenchmarkId::new("size", SIZE), &SIZE, |b, _| {
        b.to_async(Runtime::new().unwrap()).iter(|| async {
            let put_futures = (0..number_of_iterations)
                .map({
                    let client = client.clone();
                    let buckets = buckets.clone();
                    let object = object.clone();
                    move |i| {
                        task::spawn({
                            let client = client.clone();
                            let buckets = buckets.clone();
                            let object = object.clone();
                            async move {
                                client
                                    .put_object()
                                    .bucket(&buckets[i % buckets.len()])
                                    .key(&format!("test{i}"))
                                    .body(ByteStream::from(object))
                                    .send()
                                    .await
                                    .unwrap();
                            }
                        })
                    }
                })
                .collect::<Vec<_>>();
            ::futures_util::future::join_all(put_futures).await;

            let get_futures = (0..number_of_iterations)
                .map({
                    let client = client.clone();
                    let buckets = buckets.clone();
                    move |i| {
                        task::spawn({
                            let client = client.clone();
                            let buckets = buckets.clone();
                            async move {
                                client
                                    .get_object()
                                    .bucket(&buckets[i % buckets.len()])
                                    .key(&format!("test{i}"))
                                    .send()
                                    .await
                                    .unwrap();
                            }
                        })
                    }
                })
                .collect::<Vec<_>>();
            ::futures_util::future::join_all(get_futures).await;
        });
    });
    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(sample_size()).confidence_level(confidence_level());
    targets = concurrent_put_get
);
criterion_main!(benches);
