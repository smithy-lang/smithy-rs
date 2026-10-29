// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use aws_smithy_eventstream::frame::{MarshallMessage, UnmarshallMessage};
use criterion::{measurement::WallTime, BatchSize, BenchmarkGroup, BenchmarkId};
use event_stream_benchmark::{
    serde::{self, SerdeFixture},
    Case, Implementation, Protocol,
};
use std::{fmt::Debug, hint::black_box};

fn register<T, M, U>(
    group: &mut BenchmarkGroup<'_, WallTime>,
    implementation: Implementation,
    operation: &str,
    case: Case,
    fixture: SerdeFixture<T, M, U>,
) where
    T: Clone + Debug + PartialEq,
    M: MarshallMessage<Input = T>,
    U: UnmarshallMessage<Output = T>,
    U::Error: Debug,
{
    fixture.validate();
    group.bench_function(
        BenchmarkId::new(
            implementation.name(),
            format!("{}B-{}events", case.size, case.count),
        ),
        |bencher| match operation {
            "serialize" => bencher.iter_batched_ref(
                // Input clones and the result container are prepared outside timing.
                || (fixture.events.clone(), Vec::with_capacity(case.count)),
                |(events, output)| {
                    for event in events.drain(..) {
                        output.push(fixture.marshaller.marshall(black_box(event)).unwrap());
                    }
                    black_box(&*output);
                },
                BatchSize::PerIteration,
            ),
            "deserialize" => bencher.iter_batched_ref(
                || Vec::with_capacity(case.count),
                |output| {
                    for message in &fixture.messages {
                        output.push(fixture.unmarshaller.unmarshall(black_box(message)).unwrap());
                    }
                    black_box(&*output);
                },
                BatchSize::PerIteration,
            ),
            _ => unreachable!("serde operation"),
        },
    );
}

pub fn bench(
    group: &mut BenchmarkGroup<'_, WallTime>,
    implementation: Implementation,
    operation: &str,
    case: Case,
) {
    match (implementation, case.protocol) {
        (Implementation::Legacy, Protocol::RestJson) => register(
            group,
            implementation,
            operation,
            case,
            serde::rest_json_legacy::fixture(case),
        ),
        (Implementation::Schema, Protocol::RestJson) => register(
            group,
            implementation,
            operation,
            case,
            serde::rest_json_schema::fixture(case),
        ),
        (Implementation::MultiSchema, Protocol::RestJson) => register(
            group,
            implementation,
            operation,
            case,
            serde::multi_rest_json_schema::fixture(case),
        ),
        (Implementation::Legacy, Protocol::RpcV2Cbor) => register(
            group,
            implementation,
            operation,
            case,
            serde::cbor_legacy::fixture(case),
        ),
        (Implementation::Schema, Protocol::RpcV2Cbor) => register(
            group,
            implementation,
            operation,
            case,
            serde::cbor_schema::fixture(case),
        ),
        (Implementation::MultiSchema, Protocol::RpcV2Cbor) => register(
            group,
            implementation,
            operation,
            case,
            serde::multi_schema::fixture(case),
        ),
    }
}
