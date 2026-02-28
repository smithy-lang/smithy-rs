/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use sysinfo::{Pid, ProcessRefreshKind, System};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkConfig {
    pub version: u32,
    pub name: String,
    pub description: String,
    pub service: String,
    pub action: String,
    pub action_config: ActionConfig,
    pub batch: BatchConfig,
    pub warmup: WarmupConfig,
    pub measurement: MeasurementConfig,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionConfig {
    pub table_name: String,
    pub region: String,
    pub key_prefix: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchConfig {
    pub description: String,
    pub number_of_actions: usize,
    pub sequential_execution: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WarmupConfig {
    pub batches: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasurementConfig {
    pub batches: usize,
    pub collect_metrics: bool,
    pub metrics_interval: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkResults {
    pub name: String,
    pub latency_stats: LatencyStats,
    pub cpu_stats: ResourceStats,
    pub memory_stats: ResourceStats,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyStats {
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p99_ms: f64,
    pub std_dev_ms: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceStats {
    pub mean: f64,
    pub max: f64,
}

pub async fn run_benchmark(config: BenchmarkConfig) -> BenchmarkResults {
    let sdk_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new(config.action_config.region.clone()))
        .load()
        .await;
    let client = Client::new(&sdk_config);

    setup_table(&client, &config).await;

    for _ in 0..config.warmup.batches {
        run_batch(&client, &config).await;
    }

    let mut all_latencies = Vec::new();
    let mut cpu_samples = Vec::new();
    let mut memory_samples = Vec::new();

    for _ in 0..config.measurement.batches {
        let (latencies, cpu, mem) = run_batch_with_metrics(&client, &config).await;
        all_latencies.extend(latencies);
        cpu_samples.extend(cpu);
        memory_samples.extend(mem);
    }

    cleanup_table(&client, &config).await;

    BenchmarkResults {
        name: config.name,
        latency_stats: calculate_latency_stats(&all_latencies),
        cpu_stats: calculate_resource_stats(&cpu_samples),
        memory_stats: calculate_resource_stats(&memory_samples),
    }
}

async fn setup_table(client: &Client, config: &BenchmarkConfig) {
    let table_name = &config.action_config.table_name;

    let _ = client
        .create_table()
        .table_name(table_name)
        .key_schema(
            aws_sdk_dynamodb::types::KeySchemaElement::builder()
                .attribute_name("id")
                .key_type(aws_sdk_dynamodb::types::KeyType::Hash)
                .build()
                .unwrap(),
        )
        .attribute_definitions(
            aws_sdk_dynamodb::types::AttributeDefinition::builder()
                .attribute_name("id")
                .attribute_type(aws_sdk_dynamodb::types::ScalarAttributeType::S)
                .build()
                .unwrap(),
        )
        .billing_mode(aws_sdk_dynamodb::types::BillingMode::Provisioned)
        .provisioned_throughput(
            aws_sdk_dynamodb::types::ProvisionedThroughput::builder()
                .read_capacity_units(5000)
                .write_capacity_units(5000)
                .build()
                .unwrap(),
        )
        .send()
        .await;

    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    if config.action == "getitem" {
        let data = generate_1kib_data();
        for i in 0..config.batch.number_of_actions {
            let key = format!("{}{}", config.action_config.key_prefix, i);
            let mut item = HashMap::new();
            item.insert("id".to_string(), AttributeValue::S(key));
            item.insert("data".to_string(), AttributeValue::S(data.clone()));
            let _ = client
                .put_item()
                .table_name(table_name)
                .set_item(Some(item))
                .send()
                .await;
        }
    }
}

async fn cleanup_table(client: &Client, config: &BenchmarkConfig) {
    let _ = client
        .delete_table()
        .table_name(&config.action_config.table_name)
        .send()
        .await;
}

async fn run_batch(client: &Client, config: &BenchmarkConfig) {
    let data = generate_1kib_data();
    for i in 0..config.batch.number_of_actions {
        let key = format!("{}{}", config.action_config.key_prefix, i);
        match config.action.as_str() {
            "putitem" => {
                let mut item = HashMap::new();
                item.insert("id".to_string(), AttributeValue::S(key));
                item.insert("data".to_string(), AttributeValue::S(data.clone()));
                let _ = client
                    .put_item()
                    .table_name(&config.action_config.table_name)
                    .set_item(Some(item))
                    .send()
                    .await;
            }
            "getitem" => {
                let _ = client
                    .get_item()
                    .table_name(&config.action_config.table_name)
                    .key("id", AttributeValue::S(key))
                    .send()
                    .await;
            }
            _ => panic!("Unsupported action"),
        }
    }
}

async fn run_batch_with_metrics(
    client: &Client,
    config: &BenchmarkConfig,
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let mut latencies = Vec::new();
    let cpu_samples = Arc::new(Mutex::new(Vec::new()));
    let memory_samples = Arc::new(Mutex::new(Vec::new()));

    let cpu_samples_clone = cpu_samples.clone();
    let memory_samples_clone = memory_samples.clone();
    let interval = config.measurement.metrics_interval;

    let monitor = tokio::spawn(async move {
        let mut sys = System::new();
        let pid = Pid::from_u32(std::process::id());

        sys.refresh_process_specifics(pid, ProcessRefreshKind::new().with_cpu());
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        loop {
            sys.refresh_process_specifics(pid, ProcessRefreshKind::new().with_cpu().with_memory());
            if let Some(process) = sys.process(pid) {
                let cpu = process.cpu_usage() as f64;
                let mem = process.memory() as f64 / 1024.0 / 1024.0;
                cpu_samples_clone.lock().unwrap().push(cpu);
                memory_samples_clone.lock().unwrap().push(mem);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(interval)).await;
        }
    });

    let data = generate_1kib_data();
    for i in 0..config.batch.number_of_actions {
        let key = format!("{}{}", config.action_config.key_prefix, i);
        let start = Instant::now();
        match config.action.as_str() {
            "putitem" => {
                let mut item = HashMap::new();
                item.insert("id".to_string(), AttributeValue::S(key));
                item.insert("data".to_string(), AttributeValue::S(data.clone()));
                let _ = client
                    .put_item()
                    .table_name(&config.action_config.table_name)
                    .set_item(Some(item))
                    .send()
                    .await;
            }
            "getitem" => {
                let _ = client
                    .get_item()
                    .table_name(&config.action_config.table_name)
                    .key("id", AttributeValue::S(key))
                    .send()
                    .await;
            }
            _ => panic!("Unsupported action"),
        }
        latencies.push(start.elapsed().as_secs_f64() * 1000.0);
    }

    monitor.abort();

    let cpu = cpu_samples.lock().unwrap().clone();
    let mem = memory_samples.lock().unwrap().clone();

    (latencies, cpu, mem)
}

fn generate_1kib_data() -> String {
    "x".repeat(1024)
}

fn calculate_latency_stats(latencies: &[f64]) -> LatencyStats {
    let mut sorted = latencies.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let variance = sorted.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / sorted.len() as f64;
    let std_dev = variance.sqrt();

    LatencyStats {
        mean_ms: mean,
        p50_ms: percentile(&sorted, 0.5),
        p90_ms: percentile(&sorted, 0.9),
        p99_ms: percentile(&sorted, 0.99),
        std_dev_ms: std_dev,
    }
}

fn calculate_resource_stats(samples: &[f64]) -> ResourceStats {
    if samples.is_empty() {
        return ResourceStats {
            mean: 0.0,
            max: 0.0,
        };
    }
    ResourceStats {
        mean: samples.iter().sum::<f64>() / samples.len() as f64,
        max: samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let idx = (p * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx]
}
