/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::benchmark_types::{BenchmarkConfig, ResourceStats};
use super::ResourceMonitor;
use crate::bench_utils::{basic_stats, percentile};
use aws_sdk_dynamodb::client::Waiters;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionConfig {
    pub table_name: String,
    pub region: String,
    pub key_prefix: String,
    #[serde(default)]
    pub delete_table_after_benchmark: bool,
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

pub async fn run_benchmark(config: BenchmarkConfig<ActionConfig>) -> BenchmarkResults {
    let sdk_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new(config.action_config.region.clone()))
        .load()
        .await;
    let client = Client::new(&sdk_config);

    setup_table(&client, &config).await;

    for _ in 0..config.warmup.batches {
        run_batch(&client, &config).await;
    }

    let monitor = ResourceMonitor::spawn(config.measurement.metrics_interval);
    let mut all_latencies = Vec::new();

    for _ in 0..config.measurement.batches {
        let latencies = run_batch_with_latencies(&client, &config).await;
        all_latencies.extend(latencies);
    }

    let (cpu, mem) = monitor.stop();

    if config.action_config.delete_table_after_benchmark {
        if let Err(e) = client
            .delete_table()
            .table_name(&config.action_config.table_name)
            .send()
            .await
        {
            eprintln!("Warning: failed to delete table: {e}");
        } else if let Err(e) = client
            .wait_until_table_not_exists()
            .table_name(&config.action_config.table_name)
            .wait(std::time::Duration::from_secs(config.waiter_timeout_secs))
            .await
        {
            eprintln!("Warning: table deletion did not complete: {e}");
        }
    }

    BenchmarkResults {
        name: config.name,
        latency_stats: calculate_latency_stats(&all_latencies),
        cpu_stats: basic_stats(&cpu).into(),
        memory_stats: basic_stats(&mem).into(),
    }
}

async fn setup_table(client: &Client, config: &BenchmarkConfig<ActionConfig>) {
    let table_name = &config.action_config.table_name;

    let _ = client
        .create_table()
        .table_name(table_name)
        .key_schema(
            aws_sdk_dynamodb::types::KeySchemaElement::builder()
                .attribute_name("id")
                .key_type(aws_sdk_dynamodb::types::KeyType::Hash)
                .build()
                .expect("valid key schema"),
        )
        .attribute_definitions(
            aws_sdk_dynamodb::types::AttributeDefinition::builder()
                .attribute_name("id")
                .attribute_type(aws_sdk_dynamodb::types::ScalarAttributeType::S)
                .build()
                .expect("valid attribute definition"),
        )
        .billing_mode(aws_sdk_dynamodb::types::BillingMode::Provisioned)
        .provisioned_throughput(
            aws_sdk_dynamodb::types::ProvisionedThroughput::builder()
                .read_capacity_units(5000)
                .write_capacity_units(5000)
                .build()
                .expect("valid throughput"),
        )
        .send()
        .await;

    client
        .wait_until_table_exists()
        .table_name(table_name)
        .wait(std::time::Duration::from_secs(config.waiter_timeout_secs))
        .await
        .expect("table should become active");

    if config.action == "getitem" {
        let data = "x".repeat(1024);
        for i in 0..config.batch.number_of_actions {
            let key = format!("{}{}", config.action_config.key_prefix, i);
            let mut item = HashMap::new();
            item.insert("id".to_string(), AttributeValue::S(key));
            item.insert("data".to_string(), AttributeValue::S(data.clone()));
            client
                .put_item()
                .table_name(table_name)
                .set_item(Some(item))
                .send()
                .await
                .expect("seed putitem should succeed");
        }
    }
}

async fn run_batch(client: &Client, config: &BenchmarkConfig<ActionConfig>) {
    let data = "x".repeat(1024);
    for i in 0..config.batch.number_of_actions {
        let key = format!("{}{}", config.action_config.key_prefix, i);
        match config.action.as_str() {
            "putitem" => {
                let mut item = HashMap::new();
                item.insert("id".to_string(), AttributeValue::S(key));
                item.insert("data".to_string(), AttributeValue::S(data.clone()));
                client
                    .put_item()
                    .table_name(&config.action_config.table_name)
                    .set_item(Some(item))
                    .send()
                    .await
                    .expect("putitem should succeed");
            }
            "getitem" => {
                client
                    .get_item()
                    .table_name(&config.action_config.table_name)
                    .key("id", AttributeValue::S(key))
                    .send()
                    .await
                    .expect("getitem should succeed");
            }
            other => panic!("unsupported action: {other}"),
        }
    }
}

async fn run_batch_with_latencies(
    client: &Client,
    config: &BenchmarkConfig<ActionConfig>,
) -> Vec<f64> {
    let data = "x".repeat(1024);
    let mut latencies = Vec::with_capacity(config.batch.number_of_actions);

    for i in 0..config.batch.number_of_actions {
        let key = format!("{}{}", config.action_config.key_prefix, i);
        let start = Instant::now();
        match config.action.as_str() {
            "putitem" => {
                let mut item = HashMap::new();
                item.insert("id".to_string(), AttributeValue::S(key));
                item.insert("data".to_string(), AttributeValue::S(data.clone()));
                client
                    .put_item()
                    .table_name(&config.action_config.table_name)
                    .set_item(Some(item))
                    .send()
                    .await
                    .expect("putitem should succeed");
            }
            "getitem" => {
                client
                    .get_item()
                    .table_name(&config.action_config.table_name)
                    .key("id", AttributeValue::S(key))
                    .send()
                    .await
                    .expect("getitem should succeed");
            }
            other => panic!("unsupported action: {other}"),
        }
        latencies.push(start.elapsed().as_secs_f64() * 1000.0);
    }

    latencies
}

fn calculate_latency_stats(latencies: &[f64]) -> LatencyStats {
    let mut sorted = latencies.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("no NaN in latency data"));

    let stats = basic_stats(&sorted);
    LatencyStats {
        mean_ms: stats.mean,
        p50_ms: percentile(&sorted, 0.5),
        p90_ms: percentile(&sorted, 0.9),
        p99_ms: percentile(&sorted, 0.99),
        std_dev_ms: stats.std_dev,
    }
}
