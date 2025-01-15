/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_observability::attributes::{AttributeValue, Attributes};
use aws_smithy_observability::meter::{AsyncMeasure, Meter, ProvideMeter};
use aws_smithy_observability::provider::TelemetryProvider;
use aws_smithy_observability_otel::meter::{
    AsyncInstrumentWrap, AwsSdkOtelMeterProvider, MeterWrap,
};
use criterion::{criterion_group, criterion_main, Criterion};
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::runtime::Tokio;
use opentelemetry_sdk::testing::metrics::InMemoryMetricsExporter;
use std::sync::Arc;

use stats_alloc::{Region, StatsAlloc, INSTRUMENTED_SYSTEM};
use std::alloc::System;

async fn record_async_instruments(dyn_sdk_meter: Arc<MeterWrap>) {
    //Create all async instruments and record some data
    let gauge = dyn_sdk_meter.create_gauge(
        "TestGauge".to_string(),
        // Callback function records another value with different attributes so it is deduped
        |measurement: &AsyncInstrumentWrap<'_, f64>| {
            let mut attrs = Attributes::new();
            attrs.set(
                "TestGaugeAttr",
                AttributeValue::String("TestGaugeAttr".into()),
            );
            measurement.record(6.789, Some(&attrs), None);
        },
        None,
        None,
    );
    gauge.record(1.234, None, None);

    let async_ud_counter = dyn_sdk_meter.create_async_up_down_counter(
        "TestAsyncUpDownCounter".to_string(),
        |measurement: &AsyncInstrumentWrap<'_, i64>| {
            let mut attrs = Attributes::new();
            attrs.set(
                "TestAsyncUpDownCounterAttr",
                AttributeValue::String("TestAsyncUpDownCounterAttr".into()),
            );
            measurement.record(12, Some(&attrs), None);
        },
        None,
        None,
    );
    async_ud_counter.record(-6, None, None);

    let async_mono_counter = dyn_sdk_meter.create_async_monotonic_counter(
        "TestAsyncMonoCounter".to_string(),
        |measurement: &AsyncInstrumentWrap<'_, u64>| {
            let mut attrs = Attributes::new();
            attrs.set(
                "TestAsyncMonoCounterAttr",
                AttributeValue::String("TestAsyncMonoCounterAttr".into()),
            );
            measurement.record(123, Some(&attrs), None);
        },
        None,
        None,
    );
    async_mono_counter.record(4, None, None);
}

fn async_instruments_benchmark(c: &mut Criterion) {
    #[global_allocator]
    static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;
    let reg = Region::new(&GLOBAL);

    // Setup the Otel MeterProvider (which needs to be done inside an async runtime)
    // The runtime is reused later for running the bench function
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let otel_mp = runtime.block_on(async {
        let exporter = InMemoryMetricsExporter::default();
        let reader = PeriodicReader::builder(exporter.clone(), Tokio).build();
        SdkMeterProvider::builder().with_reader(reader).build()
    });

    // Create the SDK metrics types from the OTel objects
    let sdk_mp = AwsSdkOtelMeterProvider::new(otel_mp);
    let sdk_tp = TelemetryProvider::builder()
        .meter_provider(sdk_mp)
        .build()
        .unwrap();

    // Get the dyn versions of the SDK metrics objects
    let dyn_sdk_mp = sdk_tp.meter_provider();
    let dyn_sdk_meter = dyn_sdk_mp.get_meter("TestMeter", None);

    c.bench_function("async_instruments", |b| {
        b.to_async(&runtime)
            .iter(|| async { record_async_instruments(dyn_sdk_meter.clone()) });
    });
    println!("FIINISHING");
    println!("Stats at end: {:#?}", reg.change());
}

criterion_group!(benches, async_instruments_benchmark);
criterion_main!(benches);
