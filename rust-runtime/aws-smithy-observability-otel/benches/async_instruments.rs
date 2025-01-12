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

async fn record_async_instruments() {
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

    //Create all async instruments and record some data
    let gauge = dyn_sdk_meter.create_gauge(
        "TestGauge".to_string(),
        // Callback function records another value with different attributes so it is deduped
        Box::new(|measurement: &dyn AsyncMeasure<Value = f64>| {
            let mut attrs = Attributes::new();
            attrs.set(
                "TestGaugeAttr",
                AttributeValue::String("TestGaugeAttr".into()),
            );
            measurement.record(6.789, Some(&attrs), None);
        }),
        None,
        None,
    );
    gauge.record(1.234, None, None);

    let async_ud_counter = dyn_sdk_meter.create_async_up_down_counter(
        "TestAsyncUpDownCounter".to_string(),
        Box::new(|measurement: &dyn AsyncMeasure<Value = i64>| {
            let mut attrs = Attributes::new();
            attrs.set(
                "TestAsyncUpDownCounterAttr",
                AttributeValue::String("TestAsyncUpDownCounterAttr".into()),
            );
            measurement.record(12, Some(&attrs), None);
        }),
        None,
        None,
    );
    async_ud_counter.record(-6, None, None);

    let async_mono_counter = dyn_sdk_meter.create_async_monotonic_counter(
        "TestAsyncMonoCounter".to_string(),
        Box::new(|measurement: &dyn AsyncMeasure<Value = u64>| {
            let mut attrs = Attributes::new();
            attrs.set(
                "TestAsyncMonoCounterAttr",
                AttributeValue::String("TestAsyncMonoCounterAttr".into()),
            );
            measurement.record(123, Some(&attrs), None);
        }),
        None,
        None,
    );
    async_mono_counter.record(4, None, None);

    // Gracefully shutdown the metrics provider so all metrics are flushed through the pipeline
    dyn_sdk_mp
        .as_any()
        .downcast_ref::<AwsSdkOtelMeterProvider>()
        .unwrap()
        .shutdown()
        .unwrap();

    // Extract the metrics from the exporter
    let finished_metrics = exporter.get_finished_metrics().unwrap();

    // Assert that the reported metrics are what we expect
    let extracted_gauge_data = &finished_metrics[0].scope_metrics[0].metrics[0]
        .data
        .as_any()
        .downcast_ref::<Gauge<f64>>()
        .unwrap()
        .data_points[0]
        .value;

    let extracted_async_ud_counter_data = &finished_metrics[0].scope_metrics[0].metrics[1]
        .data
        .as_any()
        .downcast_ref::<Sum<i64>>()
        .unwrap()
        .data_points[0]
        .value;

    let extracted_async_mono_data = &finished_metrics[0].scope_metrics[0].metrics[2]
        .data
        .as_any()
        .downcast_ref::<Sum<u64>>()
        .unwrap()
        .data_points[0]
        .value;
}

fn async_instruments_benchmark(c: &mut Criterion) {
    c.bench_function("sync_instruments", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async { record_async_instruments() });
    });
}

criterion_group!(benches, async_instruments_benchmark);
criterion_main!(benches);
