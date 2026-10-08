/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Allocation counts for modeled-error and RPC output serialization.
//!
//! Run with `--nocapture` to see per-call allocation counts and bytes. The assertions are
//! deliberately loose sanity bounds; the printed numbers are the measurement, compared
//! before and after the response-plan caching change.

use aws_smithy_http_server::schema::protocol::{AwsJson1_1Protocol, RestJson1Protocol};
use aws_smithy_http_server::schema::{
    HttpModeledError, MetadataRoutedProtocol, OperationSchema, ProtocolBuildContext, ServerProtocol, ServiceSchema,
};
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;

struct CountingAllocator;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Allocation count and byte total across `calls` invocations of `f`, averaged per call.
fn measure(calls: u64, mut f: impl FnMut()) -> (u64, u64) {
    // Warm up any lazily initialized state so it is not attributed to the measurement.
    f();
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    let bytes = ALLOCATED_BYTES.load(Ordering::Relaxed);
    for _ in 0..calls {
        f();
    }
    (
        (ALLOCATIONS.load(Ordering::Relaxed) - allocations) / calls,
        (ALLOCATED_BYTES.load(Ordering::Relaxed) - bytes) / calls,
    )
}

const HTTP: HttpTrait<'static> = HttpTrait::new("POST", "/benchmark", Some(200));

static ERROR_TRAITS: LazyLock<aws_smithy_schema::TraitMap> = LazyLock::new(|| {
    let mut traits = aws_smithy_schema::TraitMap::new();
    traits.insert(Box::new(aws_smithy_schema::StringTrait::new(
        shape_id!("smithy.api", "error"),
        "client",
    )));
    traits
});

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
static ERRORS: [&Schema<'static>; 1] = [&BOUND_ERROR];
static OPERATION: OperationSchema<'static> =
    OperationSchema::new(shape_id!("bench", "Operation"), &INPUT, &BOUND_OUTPUT, &ERRORS);
static OPERATIONS: [&OperationSchema<'static>; 1] = [&OPERATION];
static SERVICE: ServiceSchema<'static> = ServiceSchema::new(shape_id!("bench", "Service"), None, &[], &OPERATIONS);

#[test]
fn serialization_allocation_counts() {
    let ctx = ProtocolBuildContext::new(&SERVICE);
    let rest_json = RestJson1Protocol::from_build_context(&ctx).unwrap();
    let aws_json = AwsJson1_1Protocol::from_build_context(&ctx).unwrap();

    let calls = 1000;
    let cases: [(&str, (u64, u64)); 4] = [
        (
            "rest_json_1 serialize_error (header bindings)",
            measure(calls, || {
                std::hint::black_box(rest_json.serialize_error(&BoundError));
            }),
        ),
        (
            "aws_json_1_1 serialize_error (header bindings)",
            measure(calls, || {
                std::hint::black_box(aws_json.serialize_error(&BoundError));
            }),
        ),
        (
            "rest_json_1 serialize_response (precompiled output)",
            measure(calls, || {
                std::hint::black_box(rest_json.serialize_response(&BOUND_OUTPUT, &BoundOutput));
            }),
        ),
        (
            "aws_json_1_1 serialize_response (header bindings)",
            measure(calls, || {
                std::hint::black_box(aws_json.serialize_response(&BOUND_OUTPUT, &BoundOutput));
            }),
        ),
    ];
    // Default-constructed protocols have no prepared plans: the same calls compile their plan
    // per response. The gap to the cases above is the allocation cost of plan compilation.
    let rest_json_uncached = RestJson1Protocol::default();
    let aws_json_uncached = AwsJson1_1Protocol::default();
    let uncached: [(&str, (u64, u64)); 4] = [
        (
            "rest_json_1 serialize_error (uncached plan)",
            measure(calls, || {
                std::hint::black_box(rest_json_uncached.serialize_error(&BoundError));
            }),
        ),
        (
            "aws_json_1_1 serialize_error (uncached plan)",
            measure(calls, || {
                std::hint::black_box(aws_json_uncached.serialize_error(&BoundError));
            }),
        ),
        (
            "rest_json_1 serialize_response (uncached plan)",
            measure(calls, || {
                std::hint::black_box(rest_json_uncached.serialize_response(&BOUND_OUTPUT, &BoundOutput));
            }),
        ),
        (
            "aws_json_1_1 serialize_response (uncached plan)",
            measure(calls, || {
                std::hint::black_box(aws_json_uncached.serialize_response(&BOUND_OUTPUT, &BoundOutput));
            }),
        ),
    ];
    let cases: Vec<(&str, (u64, u64))> = cases.into_iter().chain(uncached).collect();
    for (name, (allocations, bytes)) in &cases {
        eprintln!("{name}: {allocations} allocations, {bytes} bytes per call");
    }
    // Loose sanity bounds; the printed numbers are the real measurement.
    for (name, (allocations, _)) in &cases {
        assert!(*allocations < 500, "{name}: unexpectedly many allocations");
    }
}
