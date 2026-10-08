/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Modeled-error serialization and the AWS JSON output path.
//!
//! Response plans for operation outputs are precompiled by the REST protocols; modeled errors
//! and the RPC protocols compile their plans per response. These benchmarks measure that
//! per-response cost so the plan-caching change can be judged against a baseline.

use aws_smithy_http_server::response::Response;
use aws_smithy_http_server::schema::protocol::{
    AwsJson1_1Protocol, RestJson1Protocol, RestXmlProtocol, RpcV2CborProtocol,
};
use aws_smithy_http_server::schema::{
    HttpModeledError, MetadataRoutedProtocol, OperationSchema, ProtocolBuildContext, ServerProtocol, ServiceSchema,
};
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use std::sync::LazyLock;

const HTTP: HttpTrait<'static> = HttpTrait::new("POST", "/benchmark", Some(200));

static ERROR_TRAITS: LazyLock<aws_smithy_schema::TraitMap> = LazyLock::new(|| {
    let mut traits = aws_smithy_schema::TraitMap::new();
    traits.insert(Box::new(aws_smithy_schema::StringTrait::new(
        shape_id!("smithy.api", "error"),
        "client",
    )));
    traits
});

// --- an error with one body member and four header-bound members ---

static MESSAGE: Schema<'static> = Schema::new_member(
    shape_id!("bench", "BoundError", "message"),
    ShapeType::String,
    "message",
    0,
);
static CODE: Schema<'static> =
    Schema::new_member(shape_id!("bench", "BoundError", "code"), ShapeType::String, "code", 1)
        .with_http_header("x-error-code");
static TAG: Schema<'static> = Schema::new_member(shape_id!("bench", "BoundError", "tag"), ShapeType::String, "tag", 2)
    .with_http_header("x-error-tag");
static RETRY_AFTER: Schema<'static> = Schema::new_member(
    shape_id!("bench", "BoundError", "retryAfter"),
    ShapeType::Integer,
    "retryAfter",
    3,
)
.with_http_header("retry-after");
static REQUEST_ID: Schema<'static> = Schema::new_member(
    shape_id!("bench", "BoundError", "requestId"),
    ShapeType::String,
    "requestId",
    4,
)
.with_http_header("x-request-id");
static BOUND_MEMBERS: [&Schema<'static>; 5] = [&MESSAGE, &CODE, &TAG, &RETRY_AFTER, &REQUEST_ID];
static BOUND_ERROR: Schema<'static> =
    Schema::new_struct(shape_id!("bench", "BoundError"), ShapeType::Structure, &BOUND_MEMBERS)
        .with_traits(&ERROR_TRAITS);

#[derive(Debug)]
struct BoundError;

impl std::fmt::Display for BoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("bound error")
    }
}

impl std::error::Error for BoundError {}

impl SerializableStruct for BoundError {
    fn schema(&self) -> &Schema<'_> {
        &BOUND_ERROR
    }

    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        s.write_string(&MESSAGE, "throttled, slow down")?;
        s.write_string(&CODE, "Throttling")?;
        s.write_string(&TAG, "transient")?;
        s.write_integer(&RETRY_AFTER, 2)?;
        s.write_string(&REQUEST_ID, "req-0123456789")
    }
}

impl HttpModeledError for BoundError {
    fn status_code(&self) -> u16 {
        429
    }
}

// --- a control error with body members only ---

static PLAIN_MESSAGE: Schema<'static> = Schema::new_member(
    shape_id!("bench", "PlainError", "message"),
    ShapeType::String,
    "message",
    0,
);
static PLAIN_MEMBERS: [&Schema<'static>; 1] = [&PLAIN_MESSAGE];
static PLAIN_ERROR: Schema<'static> =
    Schema::new_struct(shape_id!("bench", "PlainError"), ShapeType::Structure, &PLAIN_MEMBERS)
        .with_traits(&ERROR_TRAITS);

#[derive(Debug)]
struct PlainError;

impl std::fmt::Display for PlainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("plain error")
    }
}

impl std::error::Error for PlainError {}

impl SerializableStruct for PlainError {
    fn schema(&self) -> &Schema<'_> {
        &PLAIN_ERROR
    }

    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        s.write_string(&PLAIN_MESSAGE, "plain error")
    }
}

impl HttpModeledError for PlainError {
    fn status_code(&self) -> u16 {
        400
    }
}

// --- an output with header-bound members, exercising the RPC (AWS JSON) output path ---

static OUT_TRACE: Schema<'static> = Schema::new_member(
    shape_id!("bench", "BoundOutput", "traceId"),
    ShapeType::String,
    "traceId",
    0,
)
.with_http_header("x-trace-id");
static OUT_COUNT: Schema<'static> = Schema::new_member(
    shape_id!("bench", "BoundOutput", "count"),
    ShapeType::Integer,
    "count",
    1,
);
static OUT_MEMBERS: [&Schema<'static>; 2] = [&OUT_TRACE, &OUT_COUNT];
static BOUND_OUTPUT: Schema<'static> =
    Schema::new_struct(shape_id!("bench", "BoundOutput"), ShapeType::Structure, &OUT_MEMBERS)
        .with_original_name("BoundOutput")
        .with_http(HTTP);

struct BoundOutput;

impl SerializableStruct for BoundOutput {
    fn schema(&self) -> &Schema<'_> {
        &BOUND_OUTPUT
    }

    fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        s.write_string(&OUT_TRACE, "trace-abcdef")?;
        s.write_integer(&OUT_COUNT, 42)
    }
}

static INPUT: Schema<'static> =
    Schema::new_struct(shape_id!("bench", "Input"), ShapeType::Structure, &[]).with_http(HTTP);
static ERRORS: [&Schema<'static>; 2] = [&BOUND_ERROR, &PLAIN_ERROR];
static OPERATION: OperationSchema<'static> =
    OperationSchema::new(shape_id!("bench", "Operation"), &INPUT, &BOUND_OUTPUT, &ERRORS);
static OPERATIONS: [&OperationSchema<'static>; 1] = [&OPERATION];
static SERVICE: ServiceSchema<'static> = ServiceSchema::new(shape_id!("bench", "Service"), None, &[], &OPERATIONS);

fn expect_bound(response: &Response, headers_split: bool) {
    assert_eq!(response.status().as_u16(), 429);
    // rpcv2Cbor keeps header-bound members in the error body; the others split them out.
    if headers_split {
        assert_eq!(response.headers().get("x-error-code").unwrap(), "Throttling");
        assert_eq!(response.headers().get("retry-after").unwrap(), "2");
    }
}

fn bench_protocol<P: ServerProtocol>(criterion: &mut Criterion, name: &str, protocol: P) {
    expect_bound(&protocol.serialize_error(&BoundError), !name.starts_with("rpc_v2_cbor"));
    let mut group = criterion.benchmark_group(name);
    group.bench_function("error_with_header_bindings", |bencher| {
        bencher.iter(|| black_box(black_box(&protocol).serialize_error(black_box(&BoundError))));
    });
    group.bench_function("error_body_only", |bencher| {
        bencher.iter(|| black_box(black_box(&protocol).serialize_error(black_box(&PlainError))));
    });
    group.bench_function("output_with_header_bindings", |bencher| {
        bencher.iter(|| black_box(black_box(&protocol).serialize_response(black_box(&BOUND_OUTPUT), &BoundOutput)));
    });
    group.finish();
}

fn error_bindings(criterion: &mut Criterion) {
    // Build through the same factory as a generated service, so any build-time plan
    // preparation the protocols perform is in effect, exactly as in production.
    let ctx = ProtocolBuildContext::new(&SERVICE);
    bench_protocol(
        criterion,
        "rest_json_1_errors",
        RestJson1Protocol::from_build_context(&ctx).unwrap(),
    );
    bench_protocol(
        criterion,
        "rest_xml_errors",
        RestXmlProtocol::from_build_context(&ctx).unwrap(),
    );
    bench_protocol(
        criterion,
        "aws_json_1_1",
        AwsJson1_1Protocol::from_build_context(&ctx).unwrap(),
    );
    bench_protocol(
        criterion,
        "rpc_v2_cbor",
        RpcV2CborProtocol::from_build_context(&ctx).unwrap(),
    );
    // Default-constructed protocols have no prepared plans: every schema misses the cache and
    // compiles its plan per response. The gap to the groups above is the cost of plan
    // compilation, i.e. what the build-time preparation saves.
    bench_protocol(criterion, "rest_json_1_errors_uncached", RestJson1Protocol::default());
    bench_protocol(criterion, "rest_xml_errors_uncached", RestXmlProtocol::default());
    bench_protocol(criterion, "aws_json_1_1_uncached", AwsJson1_1Protocol::default());
    bench_protocol(criterion, "rpc_v2_cbor_uncached", RpcV2CborProtocol::default());
}

criterion_group!(benches, error_bindings);
criterion_main!(benches);
