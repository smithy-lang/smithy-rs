/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::benchmark_types::{calculate_resource_stats, BenchmarkConfig, ResourceStats};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use sysinfo::{Pid, ProcessRefreshKind, System};

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
    #[serde(default = "default_cleanup_objects")]
    pub cleanup_objects_after_benchmark: bool,
}

fn default_cleanup_objects() -> bool {
    false
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
    println!(
        "Loading AWS SDK configuration for region: {}",
        config.action_config.region
    );
    let sdk_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new(config.action_config.region.clone()))
        .load()
        .await;
    let client = Client::new(&sdk_config);

    ensure_bucket_exists(&client, &config.action_config.bucket_name).await;

    let data = vec![0u8; config.action_config.object_size];
    println!(
        "Prepared {} bytes of data for operations",
        config.action_config.object_size
    );

    if config.action == "download" {
        println!(
            "Setting up {} objects for download benchmark...",
            config.batch.number_of_actions
        );
        setup_objects(&client, &config, &data).await;
        println!("Setup complete");
    }

    println!(
        "\nStarting warmup: {} batches of {} operations",
        config.warmup.batches, config.batch.number_of_actions
    );
    for i in 0..config.warmup.batches {
        println!("  Warmup batch {}/{}", i + 1, config.warmup.batches);
        run_batch(&client, &config, &data).await;
    }
    println!("Warmup complete\n");

    let cpu_samples = Arc::new(Mutex::new(Vec::new()));
    let memory_samples = Arc::new(Mutex::new(Vec::new()));
    let cpu_clone = cpu_samples.clone();
    let mem_clone = memory_samples.clone();
    let interval = config.measurement.metrics_interval;

    let monitor = tokio::spawn(async move {
        let mut sys = System::new();
        let pid = Pid::from_u32(std::process::id());
        sys.refresh_process_specifics(pid, ProcessRefreshKind::new().with_cpu());
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        loop {
            sys.refresh_process_specifics(pid, ProcessRefreshKind::new().with_cpu().with_memory());
            if let Some(process) = sys.process(pid) {
                cpu_clone.lock().unwrap().push(process.cpu_usage() as f64);
                mem_clone
                    .lock()
                    .unwrap()
                    .push(process.memory() as f64 / 1024.0 / 1024.0);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(interval)).await;
        }
    });

    println!(
        "Starting measurement: {} batches of {} operations",
        config.measurement.batches, config.batch.number_of_actions
    );
    let mut iterations = Vec::new();
    for i in 0..config.measurement.batches {
        println!(
            "  Measurement batch {}/{} starting...",
            i + 1,
            config.measurement.batches
        );
        let start = Instant::now();
        run_batch(&client, &config, &data).await;
        let total_time = start.elapsed().as_secs_f64();
        let throughput_gbps =
            (config.batch.number_of_actions as f64 * config.action_config.object_size as f64 * 8.0)
                / total_time
                / 1_000_000_000.0;
        println!(
            "  Measurement batch {}/{} complete: {:.2}s, {:.2} Gbps",
            i + 1,
            config.measurement.batches,
            total_time,
            throughput_gbps
        );
        iterations.push(IterationResult {
            total_time_seconds: total_time,
            throughput_gbps,
        });
    }
    println!("\nMeasurement complete");

    monitor.abort();

    if config.action_config.cleanup_objects_after_benchmark {
        cleanup_objects(&client, &config).await;
    }

    let cpu = cpu_samples.lock().unwrap().clone();
    let mem = memory_samples.lock().unwrap().clone();

    BenchmarkResults {
        name: config.name,
        iterations,
        cpu_stats: calculate_resource_stats(&cpu),
        memory_stats: calculate_resource_stats(&mem),
    }
}

async fn cleanup_objects(client: &Client, config: &BenchmarkConfig<ActionConfig>) {
    println!(
        "Cleaning up objects in bucket '{}'...",
        config.action_config.bucket_name
    );
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
    let _ = join_all(tasks).await;
    println!("Cleanup complete");
}

fn is_s3express_bucket(bucket: &str) -> bool {
    bucket.ends_with("--x-s3")
}

fn get_s3express_bucket_az_id(bucket: &str) -> Option<&str> {
    let parts: Vec<&str> = bucket.split("--").collect();
    if parts.len() >= 3 {
        Some(parts[parts.len() - 2])
    } else {
        None
    }
}

async fn ensure_bucket_exists(client: &Client, bucket_name: &str) {
    match client.head_bucket().bucket(bucket_name).send().await {
        Ok(_) => {
            println!("Bucket '{}' already exists", bucket_name);
            return;
        }
        Err(_) => {
            println!(
                "Bucket '{}' does not exist, attempting to create...",
                bucket_name
            );
            let mut req = client.create_bucket().bucket(bucket_name);

            if is_s3express_bucket(bucket_name) {
                // S3 Express One Zone bucket
                use aws_sdk_s3::types::{
                    BucketInfo, BucketType, CreateBucketConfiguration, DataRedundancy,
                    LocationInfo, LocationType,
                };

                if let Some(az_id) = get_s3express_bucket_az_id(bucket_name) {
                    println!("Creating S3 Express bucket in AZ: {}", az_id);
                    let location = LocationInfo::builder()
                        .r#type(LocationType::AvailabilityZone)
                        .name(az_id)
                        .build();

                    let bucket_info = BucketInfo::builder()
                        .r#type(BucketType::Directory)
                        .data_redundancy(DataRedundancy::SingleAvailabilityZone)
                        .build();

                    let config = CreateBucketConfiguration::builder()
                        .location(location)
                        .bucket(bucket_info)
                        .build();

                    req = req.create_bucket_configuration(config);
                } else {
                    panic!("Invalid S3 Express bucket name: {}", bucket_name);
                }
            } else if client.config().region().map(|r| r.as_ref()) != Some("us-east-1") {
                // Standard S3 bucket (non us-east-1)
                use aws_sdk_s3::types::{BucketLocationConstraint, CreateBucketConfiguration};
                if let Some(region) = client.config().region() {
                    let constraint = BucketLocationConstraint::from(region.as_ref());
                    let config = CreateBucketConfiguration::builder()
                        .location_constraint(constraint)
                        .build();
                    req = req.create_bucket_configuration(config);
                }
            }
            // us-east-1 standard buckets need no configuration

            match req.send().await {
                Ok(_) => {
                    println!("Bucket '{}' created successfully", bucket_name);
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                }
                Err(e) => {
                    panic!("Failed to create bucket '{}': {:?}", bucket_name, e);
                }
            }
        }
    }
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
                let _permit = sem.acquire().await.unwrap();
                fut.await
            }
        })
        .collect();
    let _ = join_all(tasks).await;
}

async fn run_batch(client: &Client, config: &BenchmarkConfig<ActionConfig>, data: &[u8]) {
    let concurrency = config
        .batch
        .concurrency
        .unwrap_or(config.batch.number_of_actions);
    let sem = Arc::new(tokio::sync::Semaphore::new(concurrency));
    if config.action == "upload" {
        println!(
            "    Starting {} upload operations (concurrency={})...",
            config.batch.number_of_actions, concurrency
        );
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
                    let _permit = sem.acquire().await.unwrap();
                    fut.await
                }
            })
            .collect();
        let _ = join_all(tasks).await;
        println!(
            "    Finished {} upload operations",
            config.batch.number_of_actions
        );
    } else {
        println!(
            "    Starting {} download operations (concurrency={})...",
            config.batch.number_of_actions, concurrency
        );
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
                    let _permit = sem.acquire().await.unwrap();
                    fut.await
                }
            })
            .collect();
        let _ = join_all(tasks).await;
        println!(
            "    Finished {} download operations",
            config.batch.number_of_actions
        );
    }
}
