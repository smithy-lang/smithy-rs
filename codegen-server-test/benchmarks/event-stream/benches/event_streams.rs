/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Measures complete generated-server processing or direct event serde for REST
//! JSON and rpcv2Cbor, comparing legacy, schema and shared-protocol services.
//! Server mode consumes complete responses; serde mode calls actual generated
//! marshallers/unmarshallers using validated fixtures and excludes HTTP and CRCs.
//!
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use event_stream_benchmark::{
    drain, hyper_in_memory, server, Case, Direction, Fixture, Implementation, Kind, Protocol,
};
use std::{hint::black_box, time::Duration};

#[path = "support/serde.rs"]
mod serde;

enum Preset {
    Small,
    Medium,
    Full,
}

impl Preset {
    fn from_env() -> Self {
        match std::env::var("BENCH_PRESET").as_deref() {
            Err(std::env::VarError::NotPresent) | Ok("small") => Self::Small,
            Ok("medium") => Self::Medium,
            Ok("full") => Self::Full,
            value => panic!("invalid BENCH_PRESET {value:?}; expected small, medium, or full"),
        }
    }

    fn workloads(&self) -> Vec<(usize, usize)> {
        let defaults: &[(usize, usize)] = match self {
            Self::Small | Self::Medium => &[(1024, 64)],
            Self::Full => &[(64, 1), (1024, 64), (1024, 1024), (65536, 16)],
        };
        let count = match std::env::var("BENCH_EVENT_COUNT") {
            Err(std::env::VarError::NotPresent) => return defaults.to_vec(),
            Ok(value) if value.is_empty() => return defaults.to_vec(),
            value => value
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|&count| count > 0 && count <= i32::MAX as usize)
                .expect("BENCH_EVENT_COUNT must be an integer between 1 and 2147483647"),
        };
        let mut workloads = Vec::new();
        for &(size, _) in defaults {
            // Full has two 1 KiB points; overriding their counts makes them identical.
            if !workloads.contains(&(size, count)) {
                workloads.push((size, count));
            }
        }
        workloads
    }

    fn implementations(&self) -> &'static [Implementation] {
        match self {
            Self::Small => &[Implementation::Legacy, Implementation::Schema],
            Self::Medium | Self::Full => &Implementation::ALL,
        }
    }
}

fn event_streams(criterion: &mut Criterion) {
    let preset = Preset::from_env();
    let mode = std::env::var("BENCH_MODE").unwrap_or_else(|_| "server".into());
    assert!(
        matches!(mode.as_str(), "server" | "serde"),
        "BENCH_MODE must be server or serde"
    );
    if mode == "serde" {
        for protocol in Protocol::ALL {
            for operation in ["serialize", "deserialize"] {
                for kind in Kind::ALL {
                    let mut group = criterion.benchmark_group(format!(
                        "serde/{}/{}/{}",
                        protocol.name(),
                        operation,
                        kind.name()
                    ));
                    for (size, count) in preset.workloads() {
                        let case = Case {
                            protocol,
                            direction: Direction::Upload,
                            kind,
                            size,
                            count,
                        };
                        group.throughput(Throughput::Elements(count as u64));
                        for &implementation in preset.implementations() {
                            serde::bench(&mut group, implementation, operation, case);
                        }
                    }
                    group.finish();
                }
            }
        }
        return;
    }
    let transport = std::env::var("BENCH_TRANSPORT").unwrap_or_else(|_| "tower".into());
    assert!(
        matches!(transport.as_str(), "tower" | "hyper-memory"),
        "BENCH_TRANSPORT must be tower or hyper-memory"
    );
    let workloads = preset.workloads();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for protocol in Protocol::ALL {
        for direction in Direction::ALL {
            for kind in Kind::ALL {
                let mut group = criterion.benchmark_group(format!(
                    "{}/{}/{}/{}",
                    transport,
                    protocol.name(),
                    direction.name(),
                    kind.name()
                ));
                // Each point measures a complete request, including all response-body polling.
                for &(size, count) in &workloads {
                    let case = Case {
                        protocol,
                        direction,
                        kind,
                        size,
                        count,
                    };
                    let fixture = Fixture::new(case);
                    group.throughput(Throughput::Elements(count as u64));
                    for &implementation in preset.implementations() {
                        let service = server(implementation, case);
                        let service = if transport == "hyper-memory" {
                            runtime.block_on(hyper_in_memory(service))
                        } else {
                            service
                        };
                        runtime.block_on(fixture.validate(service.clone()));
                        group.bench_function(
                            BenchmarkId::new(
                                implementation.name(),
                                format!("{size}B-{count}events"),
                            ),
                            |bencher| {
                                bencher.iter_batched(
                                    || (service.clone(), fixture.request()),
                                    |(service, request)| {
                                        black_box(runtime.block_on(drain(service, request)))
                                    },
                                    // Keep at most one stream's setup/output resident between iterations.
                                    BatchSize::PerIteration,
                                );
                            },
                        );
                    }
                }
                group.finish();
            }
        }
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(10))
        .measurement_time(Duration::from_secs(60))
        .sample_size(30);
    targets = event_streams
}
criterion_main!(benches);
