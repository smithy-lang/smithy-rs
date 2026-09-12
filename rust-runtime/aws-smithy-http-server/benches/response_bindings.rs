/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http_server::protocol::rest_json_1::RestJson1Protocol;
use aws_smithy_http_server::protocol::rest_xml::RestXmlProtocol;
use aws_smithy_http_server::response::Response;
use aws_smithy_http_server::schema::ServerProtocol;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use criterion::{criterion_group, criterion_main, Criterion};
use http_body_util::BodyExt;
use std::hint::black_box;

// The operation's `@http` trait is transcribed onto the output schemas by codegen.
const HTTP: HttpTrait<'static> = HttpTrait::new("GET", "/benchmark", Some(200));

static STATUS: Schema<'static> = Schema::new_member(
    shape_id!("bench", "HeaderOutput", "status"),
    ShapeType::Integer,
    "status",
    0,
)
.with_http_response_code();
static REQUEST_ID: Schema<'static> = Schema::new_member(
    shape_id!("bench", "HeaderOutput", "requestId"),
    ShapeType::String,
    "requestId",
    1,
)
.with_http_header("x-request-id");
static ETAG: Schema<'static> =
    Schema::new_member(shape_id!("bench", "HeaderOutput", "etag"), ShapeType::String, "etag", 2)
        .with_http_header("etag");
static CACHE_CONTROL: Schema<'static> = Schema::new_member(
    shape_id!("bench", "HeaderOutput", "cacheControl"),
    ShapeType::String,
    "cacheControl",
    3,
)
.with_http_header("cache-control");
static REGION: Schema<'static> = Schema::new_member(
    shape_id!("bench", "HeaderOutput", "region"),
    ShapeType::String,
    "region",
    4,
)
.with_http_header("x-region");
static VERSION: Schema<'static> = Schema::new_member(
    shape_id!("bench", "HeaderOutput", "version"),
    ShapeType::String,
    "version",
    5,
)
.with_http_header("x-version");
static HEADER_MEMBERS: [&Schema<'static>; 6] = [&STATUS, &REQUEST_ID, &ETAG, &CACHE_CONTROL, &REGION, &VERSION];
static HEADER_OUTPUT: Schema<'static> = Schema::new_struct(
    shape_id!("bench", "HeaderOutput"),
    ShapeType::Structure,
    &HEADER_MEMBERS,
)
.with_no_body_members()
.with_http(HTTP);

static TRACE_ID: Schema<'static> = Schema::new_member(
    shape_id!("bench", "MixedOutput", "traceId"),
    ShapeType::String,
    "traceId",
    0,
)
.with_http_header("x-trace-id");
static REVISION: Schema<'static> = Schema::new_member(
    shape_id!("bench", "MixedOutput", "revision"),
    ShapeType::Integer,
    "revision",
    1,
)
.with_http_header("x-revision");
static MESSAGE: Schema<'static> = Schema::new_member(
    shape_id!("bench", "MixedOutput", "message"),
    ShapeType::String,
    "message",
    2,
);
static COUNT: Schema<'static> = Schema::new_member(
    shape_id!("bench", "MixedOutput", "count"),
    ShapeType::Integer,
    "count",
    3,
);
static ENABLED: Schema<'static> = Schema::new_member(
    shape_id!("bench", "MixedOutput", "enabled"),
    ShapeType::Boolean,
    "enabled",
    4,
);
static MIXED_MEMBERS: [&Schema<'static>; 5] = [&TRACE_ID, &REVISION, &MESSAGE, &COUNT, &ENABLED];
static MIXED_OUTPUT: Schema<'static> =
    Schema::new_struct(shape_id!("bench", "MixedOutput"), ShapeType::Structure, &MIXED_MEMBERS)
        .with_original_name("MixedOutput")
        .with_http(HTTP);

static BODY_MEMBERS: [&Schema<'static>; 3] = [&MESSAGE, &COUNT, &ENABLED];
static BODY_OUTPUT: Schema<'static> =
    Schema::new_struct(shape_id!("bench", "BodyOutput"), ShapeType::Structure, &BODY_MEMBERS)
        .with_original_name("BodyOutput")
        .with_http(HTTP);

static METADATA: Schema<'static> = Schema::new_member(
    shape_id!("bench", "PrefixOutput", "metadata"),
    ShapeType::Map,
    "metadata",
    0,
)
.with_http_prefix_headers("x-meta-");
static PREFIX_MEMBERS: [&Schema<'static>; 1] = [&METADATA];
static PREFIX_OUTPUT: Schema<'static> = Schema::new_struct(
    shape_id!("bench", "PrefixOutput"),
    ShapeType::Structure,
    &PREFIX_MEMBERS,
)
.with_no_body_members()
.with_http(HTTP);

struct HeaderOutput;

impl SerializableStruct for HeaderOutput {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        serializer.write_integer(&STATUS, 202)?;
        serializer.write_string(&REQUEST_ID, "req-0123456789")?;
        serializer.write_string(&ETAG, "\"v1\"")?;
        serializer.write_string(&CACHE_CONTROL, "public, max-age=60")?;
        serializer.write_string(&REGION, "us-west-2")?;
        serializer.write_string(&VERSION, "2026-09-10")
    }
}

struct MixedOutput;

impl SerializableStruct for MixedOutput {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        serializer.write_string(&TRACE_ID, "trace-abcdef")?;
        serializer.write_integer(&REVISION, 17)?;
        write_body(serializer)
    }
}

struct BodyOutput;

impl SerializableStruct for BodyOutput {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        write_body(serializer)
    }
}

fn write_body(serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
    serializer.write_string(&MESSAGE, "response serialization benchmark")?;
    serializer.write_integer(&COUNT, 42)?;
    serializer.write_boolean(&ENABLED, true)
}

struct PrefixOutput;

impl SerializableStruct for PrefixOutput {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        serializer.write_map(&METADATA, &|serializer| {
            serializer.write_string(&aws_smithy_schema::prelude::STRING, "color")?;
            serializer.write_string(&aws_smithy_schema::prelude::STRING, "blue")?;
            serializer.write_string(&aws_smithy_schema::prelude::STRING, "build")?;
            serializer.write_string(&aws_smithy_schema::prelude::STRING, "release")?;
            serializer.write_string(&aws_smithy_schema::prelude::STRING, "owner")?;
            serializer.write_string(&aws_smithy_schema::prelude::STRING, "smithy")
        })
    }
}

struct Expected<'a> {
    status: u16,
    headers: &'a [(&'a str, &'a str)],
    content_type: Option<&'a str>,
    body: &'a str,
}

fn validate(response: Response, expected: Expected<'_>) {
    assert_eq!(response.status().as_u16(), expected.status);
    for (name, value) in expected.headers {
        assert_eq!(response.headers().get(*name).expect("expected header"), *value);
    }
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .map(|value| value.to_str().unwrap()),
        expected.content_type
    );
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let body = runtime.block_on(response.into_body().collect()).unwrap().to_bytes();
    assert_eq!(body.as_ref(), expected.body.as_bytes());
}

fn bench_protocol<P: ServerProtocol>(criterion: &mut Criterion, protocol_name: &str, protocol: P) {
    // restJson1 labels every response `application/json`, restXml only codec bodies.
    let empty_content_type = match protocol_name {
        "rest_json_1" => Some("application/json"),
        "rest_xml" => None,
        _ => unreachable!(),
    };
    let content_type = match protocol_name {
        "rest_json_1" => "application/json",
        "rest_xml" => "application/xml",
        _ => unreachable!(),
    };
    let mixed_body = match protocol_name {
        "rest_json_1" => r#"{"message":"response serialization benchmark","count":42,"enabled":true}"#,
        "rest_xml" => "<MixedOutput><message>response serialization benchmark</message><count>42</count><enabled>true</enabled></MixedOutput>",
        _ => unreachable!(),
    };
    let body_body = match protocol_name {
        "rest_json_1" => r#"{"message":"response serialization benchmark","count":42,"enabled":true}"#,
        "rest_xml" => "<BodyOutput><message>response serialization benchmark</message><count>42</count><enabled>true</enabled></BodyOutput>",
        _ => unreachable!(),
    };

    validate(
        protocol.serialize_response(&HEADER_OUTPUT, &HeaderOutput),
        Expected {
            status: 202,
            headers: &[
                ("x-request-id", "req-0123456789"),
                ("etag", "\"v1\""),
                ("cache-control", "public, max-age=60"),
                ("x-region", "us-west-2"),
                ("x-version", "2026-09-10"),
            ],
            content_type: empty_content_type,
            body: "",
        },
    );
    validate(
        protocol.serialize_response(&MIXED_OUTPUT, &MixedOutput),
        Expected {
            status: 200,
            headers: &[("x-trace-id", "trace-abcdef"), ("x-revision", "17")],
            content_type: Some(content_type),
            body: mixed_body,
        },
    );
    validate(
        protocol.serialize_response(&BODY_OUTPUT, &BodyOutput),
        Expected {
            status: 200,
            headers: &[],
            content_type: Some(content_type),
            body: body_body,
        },
    );
    validate(
        protocol.serialize_response(&PREFIX_OUTPUT, &PrefixOutput),
        Expected {
            status: 200,
            headers: &[
                ("x-meta-color", "blue"),
                ("x-meta-build", "release"),
                ("x-meta-owner", "smithy"),
            ],
            content_type: empty_content_type,
            body: "",
        },
    );

    let mut group = criterion.benchmark_group(protocol_name);
    group.bench_function("five_headers_and_status", |bencher| {
        bencher.iter(|| {
            let response = black_box(&protocol).serialize_response(black_box(&HEADER_OUTPUT), black_box(&HeaderOutput));
            black_box(response)
        });
    });
    group.bench_function("mixed_headers_and_codec_body", |bencher| {
        bencher.iter(|| {
            let response = black_box(&protocol).serialize_response(black_box(&MIXED_OUTPUT), black_box(&MixedOutput));
            black_box(response)
        });
    });
    group.bench_function("codec_body_only", |bencher| {
        bencher.iter(|| {
            let response = black_box(&protocol).serialize_response(black_box(&BODY_OUTPUT), black_box(&BodyOutput));
            black_box(response)
        });
    });
    group.bench_function("dynamic_prefix_headers", |bencher| {
        bencher.iter(|| {
            let response = black_box(&protocol).serialize_response(black_box(&PREFIX_OUTPUT), black_box(&PrefixOutput));
            black_box(response)
        });
    });
    group.finish();
}

fn response_bindings(criterion: &mut Criterion) {
    bench_protocol(criterion, "rest_json_1", RestJson1Protocol::default());
    bench_protocol(criterion, "rest_xml", RestXmlProtocol::default());
}

criterion_group!(benches, response_bindings);
criterion_main!(benches);
