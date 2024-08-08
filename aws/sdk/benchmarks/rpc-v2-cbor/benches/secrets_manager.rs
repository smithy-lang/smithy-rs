/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_secretsmanager::config::BehaviorVersion;
use aws_sdk_secretsmanager::primitives::Blob;
use aws_sdk_secretsmanager::Client;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use rand::distributions::Alphanumeric;
use rand::Rng;
use std::iter;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;

const KiB: usize = 1024;

// Configures the sample size for benchmarks.
// If an environment variable `SAMPLE_SIZE` is not set, it falls back to Criterion's default.
fn sample_size() -> usize {
    let sample_size = std::env::var("SAMPLE_SIZE").map_or(100, |s| s.parse::<usize>().unwrap());
    dbg!(sample_size)
}

// Configures the measurement time for benchmarks.
// If an environment variable `MEASUREMENT_TIME_SECS` is not set, it falls back to Criterion's default.
fn measurement_time() -> std::time::Duration {
    let measurement_time = std::env::var("MEASUREMENT_TIME_SECS")
        .map_or(std::time::Duration::from_secs(5), |t| {
            std::time::Duration::from_secs(t.parse::<u64>().unwrap())
        });
    dbg!(measurement_time)
}

fn random_string(rng: &mut impl Rng, size: usize) -> String {
    iter::repeat_with(|| rng.sample(Alphanumeric))
        .take(size)
        .map(char::from)
        .collect()
}

fn random_binary(rng: &mut impl Rng, size: usize) -> Vec<u8> {
    let mut binary_vec = Vec::with_capacity(size);
    for _ in 0..size {
        let bit = rng.gen_range(0..2); // Generate either 0 or 1
        binary_vec.push(bit);
    }
    binary_vec
}

async fn clean_up_secrets(client: &Client, prefix: &str) {
    let mut stream = client.list_secrets().into_paginator().send();
    while let Some(page) = stream.next().await {
        let secret_list = page.unwrap().secret_list.unwrap();
        for s in &secret_list {
            if let Some(name) = &s.name {
                if name.starts_with(prefix) {
                    let _ = client.delete_secret().secret_id(name).send().await.unwrap();
                }
            }
        }
    }
}

pub fn create_secret(c: &mut Criterion) {
    let client = Runtime::new().unwrap().block_on(async {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        Client::new(&config)
    });

    let mut group = c.benchmark_group("create_secret");

    for size in [KiB, 8 * KiB, 64 * KiB].iter() {
        group.bench_with_input(BenchmarkId::new("text", size), size, |b, _| {
            b.to_async(Runtime::new().unwrap()).iter(|| async {
                let mut rng = rand::thread_rng();
                let _ = client
                    .create_secret()
                    .name(format!(
                        "create-secret-text-{}-{}",
                        size,
                        random_string(&mut rng, 10)
                    ))
                    .secret_string(random_string(&mut rng, *size))
                    .send()
                    .await
                    .unwrap();
            });
        });

        group.bench_with_input(BenchmarkId::new("binary", size), size, |b, _| {
            b.to_async(Runtime::new().unwrap()).iter(|| async {
                let mut rng = rand::thread_rng();
                let _ = client
                    .create_secret()
                    .name(format!(
                        "create-secret-binary-{}-{}",
                        size,
                        random_string(&mut rng, 10)
                    ))
                    .secret_binary(Blob::new(random_binary(&mut rng, *size)))
                    .send()
                    .await
                    .unwrap();
            });
        });
    }
    group.finish();

    // Clean up test secrets
    Runtime::new()
        .unwrap()
        .block_on(clean_up_secrets(&client, "create-secret-"));
}

pub fn get_secret_value(c: &mut Criterion) {
    let current_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("should get the current Unix epoch time")
        .as_secs();

    // Set up secrets used by the benchmark
    Runtime::new().unwrap().block_on(async {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        let client = aws_sdk_secretsmanager::Client::new(&config);
        let mut rng = rand::thread_rng();
        for size in [KiB, 8 * KiB, 64 * KiB].iter() {
            let _ = client
                .create_secret()
                .name(format!("get-secret-value-text-{size}-{current_epoch}"))
                .secret_string(random_string(&mut rng, *size))
                .send()
                .await
                .unwrap();

            let _ = client
                .create_secret()
                .name(format!("get-secret-value-binary-{size}-{current_epoch}"))
                .secret_binary(Blob::new(random_binary(&mut rng, *size)))
                .send()
                .await
                .unwrap();
        }
    });

    // Create a fresh, new client for the benchmark
    let client = Runtime::new().unwrap().block_on(async {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        Client::new(&config)
    });

    let mut group = c.benchmark_group("get_secret_value");

    for size in [KiB, 8 * KiB, 64 * KiB].iter() {
        group.bench_with_input(BenchmarkId::new("text", size), size, |b, _| {
            b.to_async(Runtime::new().unwrap()).iter(|| async {
                let _ = client
                    .get_secret_value()
                    .secret_id(format!("get-secret-value-text-{size}-{current_epoch}"))
                    .send()
                    .await
                    .unwrap();
            });
        });

        group.bench_with_input(BenchmarkId::new("binary", size), size, |b, _| {
            b.to_async(Runtime::new().unwrap()).iter(|| async {
                let _ = client
                    .get_secret_value()
                    .secret_id(format!("get-secret-value-binary-{size}-{current_epoch}"))
                    .send()
                    .await
                    .unwrap();
            });
        });
    }
    group.finish();

    // Clean up test secrets
    Runtime::new()
        .unwrap()
        .block_on(clean_up_secrets(&client, "get-secret-value-"));
}

criterion_group!(
    name = group1;
    config = Criterion::default().sample_size(sample_size()).measurement_time(measurement_time());
    targets = create_secret
);
criterion_group!(
    name = group2;
    config = Criterion::default().sample_size(sample_size()).measurement_time(measurement_time());
    targets = get_secret_value
);
criterion_main!(group1, group2);
