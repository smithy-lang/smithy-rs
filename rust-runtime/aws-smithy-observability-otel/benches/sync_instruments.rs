/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::sync::Arc;

use aws_smithy_observability::attributes::{AttributeValue, Attributes};
use aws_smithy_observability::meter::{
    AsyncMeasure, Histogram, Meter, MonotonicCounter, ProvideMeter, UpDownCounter,
};
use aws_smithy_observability::provider::TelemetryProvider;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use opentelemetry_sdk::metrics::{
    data::{Gauge, Histogram as OtelHistogram, Sum},
    PeriodicReader, SdkMeterProvider,
};
use opentelemetry_sdk::runtime::Tokio;
use opentelemetry_sdk::testing::metrics::InMemoryMetricsExporter;

use aws_smithy_observability_otel::meter::AwsSdkOtelMeterProvider;

async fn record_sync_instruments() {
    // Create the OTel metrics objects
    let exporter = InMemoryMetricsExporter::default();
    let reader = PeriodicReader::builder(exporter.clone(), Tokio).build();
    let otel_mp = SdkMeterProvider::builder().with_reader(reader).build();

    // Create the SDK metrics types from the OTel objects
    let sdk_mp = AwsSdkOtelMeterProvider::new(otel_mp);
    let sdk_tp = TelemetryProvider::builder().meter_provider(sdk_mp).build();

    // Get the dyn versions of the SDK metrics objects
    let dyn_sdk_mp = sdk_tp.meter_provider();
    let dyn_sdk_meter = dyn_sdk_mp.get_meter("TestMeter", None);

    //Create all 3 sync instruments and record some data for each
    let mono_counter =
        dyn_sdk_meter.create_monotonic_counter("TestMonoCounter".to_string(), None, None);
    mono_counter.add(4, None, None);
    let ud_counter =
        dyn_sdk_meter.create_up_down_counter("TestUpDownCounter".to_string(), None, None);
    ud_counter.add(-6, None, None);
    let histogram = dyn_sdk_meter.create_histogram("TestHistogram".to_string(), None, None);
    histogram.record(1.234, None, None);

    // Gracefully shutdown the metrics provider so all metrics are flushed through the pipeline
    dyn_sdk_mp
        .as_any()
        .downcast_ref::<AwsSdkOtelMeterProvider>()
        .unwrap()
        .shutdown()
        .unwrap();

    // Extract the metrics from the exporter and assert that they are what we expect
    let finished_metrics = exporter.get_finished_metrics().unwrap();
    let extracted_mono_counter_data = &finished_metrics[0].scope_metrics[0].metrics[0]
        .data
        .as_any()
        .downcast_ref::<Sum<u64>>()
        .unwrap()
        .data_points[0]
        .value;

    let extracted_ud_counter_data = &finished_metrics[0].scope_metrics[0].metrics[1]
        .data
        .as_any()
        .downcast_ref::<Sum<i64>>()
        .unwrap()
        .data_points[0]
        .value;

    let extracted_histogram_data = &finished_metrics[0].scope_metrics[0].metrics[2]
        .data
        .as_any()
        .downcast_ref::<OtelHistogram<f64>>()
        .unwrap()
        .data_points[0]
        .sum;
}

fn sync_instruments_benchmark(c: &mut Criterion) {
    c.bench_function("sync_instruments", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async { record_sync_instruments() });
    });
}

criterion_group!(benches, sync_instruments_benchmark);
criterion_main!(benches);
