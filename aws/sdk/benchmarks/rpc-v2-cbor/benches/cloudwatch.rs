/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sdk_cloudwatch::config::BehaviorVersion;
use aws_sdk_cloudwatch::primitives::DateTime;
use aws_sdk_cloudwatch::types::{Metric, MetricDataQuery, MetricStat};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::ops::Mul;
use std::time::{Duration, SystemTime};
use tokio::runtime::Runtime;

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

pub fn get_metric_data(c: &mut Criterion) {
    let client = Runtime::new().unwrap().block_on(async {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        aws_sdk_cloudwatch::Client::new(&config)
    });

    let mut group = c.benchmark_group("get_metric_data");

    let day = Duration::from_secs(3600 * 24);
    let week = day.mul(7);
    let month = week.mul(4);

    let end_time = SystemTime::now();

    for how_far_back in [day, week, month].iter() {
        group.bench_with_input(
            BenchmarkId::new("duration", format!("{how_far_back:?}")),
            how_far_back,
            |b, _| {
                b.to_async(Runtime::new().unwrap()).iter(|| async {
                    let metric = Metric::builder()
                        .namespace("AWS/Lambda".to_string())
                        .metric_name("Duration".to_string())
                        .set_dimensions(None)
                        .build();

                    let metric_stat = MetricStat::builder()
                        .metric(metric)
                        .period(60)
                        .stat("Average")
                        .build();

                    let usage_data = MetricDataQuery::builder()
                        .metric_stat(metric_stat)
                        .id("d")
                        .return_data(true)
                        .build();

                    let _ = client
                        .get_metric_data()
                        .metric_data_queries(usage_data)
                        .start_time(DateTime::from(end_time.checked_sub(*how_far_back).unwrap()))
                        .end_time(DateTime::from(end_time))
                        .into_paginator()
                        .send()
                        .collect::<Result<Vec<_>, _>>()
                        .await
                        .unwrap();
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(sample_size()).measurement_time(measurement_time());
    targets = get_metric_data
);
criterion_main!(benches);
