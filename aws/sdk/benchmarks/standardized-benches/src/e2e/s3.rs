/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::benchmark_types::{BenchmarkConfig, ResourceStats};
use super::ResourceMonitor;
use crate::bench_utils::basic_stats;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use aws_smithy_types::error::display::DisplayErrorContext;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionConfig {
    pub bucket_name: String,
    pub region: String,
    pub object_size: usize,
    pub key_prefix: String,
    #[serde(default)]
    pub files_on_disk: bool,
    pub checksum: Option<String>,
    #[serde(default)]
    pub cleanup_objects_after_benchmark: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkResults {
    pub name: String,
    pub iterations: Vec<IterationResult>,
    pub cpu_stats: ResourceStats,
    pub memory_stats: ResourceStats,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IterationResult {
    pub total_time_seconds: f64,
    pub throughput_gbps: f64,
}

pub async fn run_benchmark(config: BenchmarkConfig<ActionConfig>) -> BenchmarkResults {
    let mut config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new(config.action_config.region.clone()));
    if config.action_config.checksum.is_none() {
        config_loader = config_loader
            .request_checksum_calculation(
                aws_sdk_s3::config::RequestChecksumCalculation::WhenRequired,
            )
            .response_checksum_validation(
                aws_sdk_s3::config::ResponseChecksumValidation::WhenRequired,
            );
    }
    let sdk_config = config_loader.load().await;
    let client = Client::new(&sdk_config);

    ensure_bucket_exists(&client, &config).await;

    let data = vec![0u8; config.action_config.object_size];

    if config.action == "download" {
        setup_objects(&client, &config, &data).await;
    }

    for _ in 0..config.warmup.batches {
        run_batch(&client, &config, &data).await;
    }

    let monitor = ResourceMonitor::spawn(config.measurement.metrics_interval);
    let mut iterations = Vec::new();

    for _ in 0..config.measurement.batches {
        let start = Instant::now();
        run_batch(&client, &config, &data).await;
        let total_time = start.elapsed().as_secs_f64();
        let throughput_gbps =
            (config.batch.number_of_actions as f64 * config.action_config.object_size as f64 * 8.0)
                / total_time
                / 1_000_000_000.0;
        iterations.push(IterationResult {
            total_time_seconds: total_time,
            throughput_gbps,
        });
    }

    let (cpu, mem) = monitor.stop();

    if config.action_config.cleanup_objects_after_benchmark {
        cleanup_objects(&client, &config).await;
    }

    BenchmarkResults {
        name: config.name,
        iterations,
        cpu_stats: basic_stats(&cpu).into(),
        memory_stats: basic_stats(&mem).into(),
    }
}

fn assert_all_ok<T, E: std::fmt::Debug + std::error::Error>(
    results: &[Result<T, E>],
    label: &str,
    expected: usize,
) {
    let ok_count = results.iter().filter(|r| r.is_ok()).count();
    if ok_count != expected {
        if let Some(first_err) = results.iter().find_map(|r| r.as_ref().err()) {
            eprintln!("  First error: {}", DisplayErrorContext(first_err));
        }
        panic!(
            "{}: only {}/{} operations succeeded",
            label, ok_count, expected
        );
    }
}

async fn cleanup_objects(client: &Client, config: &BenchmarkConfig<ActionConfig>) {
    let tasks: Vec<_> = (0..config.batch.number_of_actions)
        .map(|i| {
            let key = format!("{}{}", config.action_config.key_prefix, i);
            client
                .delete_object()
                .bucket(&config.action_config.bucket_name)
                .key(key)
                .send()
        })
        .collect();
    let results = join_all(tasks).await;
    let ok_count = results.iter().filter(|r| r.is_ok()).count();
    if ok_count != config.batch.number_of_actions {
        eprintln!(
            "  Warning: cleanup {}/{} succeeded",
            ok_count, config.batch.number_of_actions
        );
    }
}

async fn ensure_bucket_exists(client: &Client, config: &BenchmarkConfig<ActionConfig>) {
    let bucket_name = &config.action_config.bucket_name;
    let waiter_timeout = std::time::Duration::from_secs(config.waiter_timeout_secs);

    if client
        .head_bucket()
        .bucket(bucket_name)
        .send()
        .await
        .is_ok()
    {
        return;
    }

    let mut req = client.create_bucket().bucket(bucket_name);

    if bucket_name.ends_with("--x-s3") {
        use aws_sdk_s3::types::{
            BucketInfo, BucketType, CreateBucketConfiguration, DataRedundancy, LocationInfo,
            LocationType,
        };
        let parts: Vec<&str> = bucket_name.split("--").collect();
        let az_id = parts[parts.len() - 2];
        let config = CreateBucketConfiguration::builder()
            .location(
                LocationInfo::builder()
                    .r#type(LocationType::AvailabilityZone)
                    .name(az_id)
                    .build(),
            )
            .bucket(
                BucketInfo::builder()
                    .r#type(BucketType::Directory)
                    .data_redundancy(DataRedundancy::SingleAvailabilityZone)
                    .build(),
            )
            .build();
        req = req.create_bucket_configuration(config);
    } else if client.config().region().map(|r| r.as_ref()) != Some("us-east-1") {
        use aws_sdk_s3::types::{BucketLocationConstraint, CreateBucketConfiguration};
        if let Some(region) = client.config().region() {
            let config = CreateBucketConfiguration::builder()
                .location_constraint(BucketLocationConstraint::from(region.as_ref()))
                .build();
            req = req.create_bucket_configuration(config);
        }
    }

    req.send()
        .await
        .unwrap_or_else(|e| panic!("failed to create bucket '{}': {:?}", bucket_name, e));

    use aws_sdk_s3::client::Waiters;
    client
        .wait_until_bucket_exists()
        .bucket(bucket_name)
        .wait(waiter_timeout)
        .await
        .unwrap_or_else(|e| panic!("timed out waiting for bucket '{}': {:?}", bucket_name, e));
}

async fn setup_objects(client: &Client, config: &BenchmarkConfig<ActionConfig>, data: &[u8]) {
    let concurrency = config
        .batch
        .concurrency
        .unwrap_or(config.batch.number_of_actions);
    let sem = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let tasks: Vec<_> = (0..config.batch.number_of_actions)
        .map(|i| {
            let key = format!("{}{}", config.action_config.key_prefix, i);
            let body = ByteStream::from(data.to_vec());
            let sem = sem.clone();
            let fut = client
                .put_object()
                .bucket(&config.action_config.bucket_name)
                .key(key)
                .body(body)
                .send();
            async move {
                let _permit = sem.acquire().await.expect("semaphore should not be closed");
                fut.await
            }
        })
        .collect();
    let results = join_all(tasks).await;
    assert_all_ok(&results, "setup_objects", config.batch.number_of_actions);
}

async fn run_batch(client: &Client, config: &BenchmarkConfig<ActionConfig>, data: &[u8]) {
    let concurrency = config
        .batch
        .concurrency
        .unwrap_or(config.batch.number_of_actions);
    let sem = Arc::new(tokio::sync::Semaphore::new(concurrency));

    if config.action == "upload" {
        let tasks: Vec<_> = (0..config.batch.number_of_actions)
            .map(|i| {
                let key = format!("{}{}", config.action_config.key_prefix, i);
                let body = ByteStream::from(data.to_vec());
                let sem = sem.clone();
                let fut = client
                    .put_object()
                    .bucket(&config.action_config.bucket_name)
                    .key(key)
                    .body(body)
                    .send();
                async move {
                    let _permit = sem.acquire().await.expect("semaphore should not be closed");
                    fut.await
                }
            })
            .collect();
        let results = join_all(tasks).await;
        assert_all_ok(&results, "upload", config.batch.number_of_actions);
    } else {
        let tasks: Vec<_> = (0..config.batch.number_of_actions)
            .map(|i| {
                let key = format!("{}{}", config.action_config.key_prefix, i);
                let sem = sem.clone();
                let fut = client
                    .get_object()
                    .bucket(&config.action_config.bucket_name)
                    .key(key)
                    .send();
                async move {
                    let _permit = sem.acquire().await.expect("semaphore should not be closed");
                    let resp = fut.await.expect("download request should succeed");
                    resp.body
                        .collect()
                        .await
                        .expect("download body read should succeed");
                }
            })
            .collect();
        join_all(tasks).await;
    }
}
