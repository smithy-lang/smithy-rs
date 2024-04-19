/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_config::BehaviorVersion;
use aws_sdk_s3::primitives::ByteStream;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use s3_express::{confidence_level, number_of_iterations, sample_size};
use tokio::runtime::Runtime;

async fn prepare_test_objects<T: AsRef<str>>(buckets: &[T], object_sizes: &[usize]) {
    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let client = aws_sdk_s3::Client::new(&config);

    for size in object_sizes {
        let object: Vec<u8> = vec![0; *size];
        for bucket in buckets.as_ref() {
            client
                .put_object()
                .bucket(bucket.as_ref())
                .key(&format!("test-{size}"))
                .body(ByteStream::from(object.clone()))
                .send()
                .await
                .unwrap();
        }
    }
}

pub fn get_object(c: &mut Criterion) {
    let buckets = if let Ok(buckets) = std::env::var("BUCKETS") {
        buckets.split(",").map(String::from).collect::<Vec<_>>()
    } else {
        panic!("required environment variable `BUCKETS` should be set: e.g. `BUCKETS=\"bucket1,bucket2\"`")
    };

    let number_of_iterations = number_of_iterations();

    println!(
        "measuring {number_of_iterations} of GetObject against {buckets:#?}, \
        switching buckets on every operation if more than one bucket is specified\n"
    );

    const KB: usize = 1024;
    let sizes = [64 * KB, KB * KB];

    let client = Runtime::new().unwrap().block_on(async {
        prepare_test_objects(&buckets, &sizes).await;

        // Return a new client that has an empty identity cache
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        aws_sdk_s3::Client::new(&config)
    });

    let mut group = c.benchmark_group("get_object");

    for size in sizes.iter() {
        let key = format!("test-{size}");
        group.bench_with_input(BenchmarkId::new("size", size), size, |b, _| {
            b.to_async(Runtime::new().unwrap()).iter(|| async {
                for i in 0..number_of_iterations {
                    let bucket = &buckets[i % buckets.len()];
                    client
                        .get_object()
                        .bucket(bucket)
                        .key(&key)
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
        for size in sizes {
            let key = format!("test-{size}");
            for bucket in &buckets {
                client
                    .delete_object()
                    .bucket(bucket)
                    .key(&key)
                    .send()
                    .await
                    .unwrap();
            }
        }
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(sample_size()).confidence_level(confidence_level());
    targets = get_object
);
criterion_main!(benches);
