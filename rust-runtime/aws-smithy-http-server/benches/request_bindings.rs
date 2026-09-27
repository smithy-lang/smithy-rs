/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! CPU and allocation benchmarks for the REST request-binding deserializer
//! (`schema/request_bindings.rs`), driven through the public
//! [`ServerProtocol::deserialize_request`] path exactly as generated servers do.
//!
//! Label, query, header, and prefix-header scenarios run under restJson1 only:
//! those paths never touch the codec, so the numbers are identical for every
//! REST protocol. The codec-delegation baseline (`body_only`) runs under both
//! restJson1 and restXml.
//!
//! Alongside the criterion timing groups, a memory report prints the number of
//! heap allocations and total bytes allocated per request for each scenario,
//! measured by a counting global allocator. The counter costs two relaxed
//! atomic increments per allocation, a small constant tax on the timing
//! numbers that applies equally to every scenario.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

use aws_smithy_http_server::protocol::rest_json_1::RestJson1Protocol;
use aws_smithy_http_server::protocol::rest_xml::RestXmlProtocol;
use aws_smithy_http_server::schema::{ServerProtocol, ServerRequest};
use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use bytes::Bytes;
use criterion::{criterion_group, criterion_main, Criterion};

// ============================================================================
// Counting allocator
// ============================================================================

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

// SAFETY: delegates every operation to `System` unchanged; only counters are added.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(new_size.saturating_sub(layout.size()), Ordering::Relaxed);
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

// ============================================================================
// Schemas — one static member set per scenario, mirroring what codegen emits.
// The operation's `@http` trait is transcribed onto the input schema by codegen.
// ============================================================================

// --- label_plain: POST /pets/{name} ---
static L_NAME: Schema<'static> =
    Schema::new_member(shape_id!("bench", "LabelInput", "name"), ShapeType::String, "name", 0).with_http_label();
static LABEL_MEMBERS: [&Schema<'static>; 1] = [&L_NAME];
static LABEL_INPUT: Schema<'static> =
    Schema::new_struct(shape_id!("bench", "LabelInput"), ShapeType::Structure, &LABEL_MEMBERS)
        .with_http(HttpTrait::new("POST", "/pets/{name}", Some(200)));

// --- label_greedy: POST /data/{key+}/meta ---
static G_KEY: Schema<'static> =
    Schema::new_member(shape_id!("bench", "GreedyInput", "key"), ShapeType::String, "key", 0).with_http_label();
static GREEDY_MEMBERS: [&Schema<'static>; 1] = [&G_KEY];
static GREEDY_INPUT: Schema<'static> =
    Schema::new_struct(shape_id!("bench", "GreedyInput"), ShapeType::Structure, &GREEDY_MEMBERS)
        .with_http(HttpTrait::new("POST", "/data/{key+}/meta", Some(200)));

// --- query: scalar `@httpQuery` + list `@httpQuery` ---
static Q_AGE: Schema<'static> =
    Schema::new_member(shape_id!("bench", "QueryInput", "age"), ShapeType::Integer, "age", 0).with_http_query("age");
static Q_TAGS: Schema<'static> =
    Schema::new_member(shape_id!("bench", "QueryInput", "tags"), ShapeType::List, "tags", 1).with_http_query("tag");
static QUERY_MEMBERS: [&Schema<'static>; 2] = [&Q_AGE, &Q_TAGS];
static QUERY_INPUT: Schema<'static> =
    Schema::new_struct(shape_id!("bench", "QueryInput"), ShapeType::Structure, &QUERY_MEMBERS)
        .with_http(HttpTrait::new("POST", "/things", Some(200)));

// --- query_params_map: `@httpQueryParams` + one explicit `@httpQuery` ---
static QP_PARAMS: Schema<'static> = Schema::new_member(
    shape_id!("bench", "QueryParamsInput", "params"),
    ShapeType::Map,
    "params",
    0,
)
.with_http_query_params();
static QP_AGE: Schema<'static> = Schema::new_member(
    shape_id!("bench", "QueryParamsInput", "age"),
    ShapeType::Integer,
    "age",
    1,
)
.with_http_query("age");
static QP_MEMBERS: [&Schema<'static>; 2] = [&QP_PARAMS, &QP_AGE];
static QP_INPUT: Schema<'static> = Schema::new_struct(
    shape_id!("bench", "QueryParamsInput"),
    ShapeType::Structure,
    &QP_MEMBERS,
)
.with_http(HttpTrait::new("POST", "/things", Some(200)));

// --- headers: string, tokenized integer, timestamp, integer list ---
static H_TOKEN: Schema<'static> = Schema::new_member(
    shape_id!("bench", "HeaderInput", "token"),
    ShapeType::String,
    "token",
    0,
)
.with_http_header("x-token");
static H_COUNT: Schema<'static> = Schema::new_member(
    shape_id!("bench", "HeaderInput", "count"),
    ShapeType::Integer,
    "count",
    1,
)
.with_http_header("x-count");
static H_WHEN: Schema<'static> = Schema::new_member(
    shape_id!("bench", "HeaderInput", "when"),
    ShapeType::Timestamp,
    "when",
    2,
)
.with_http_header("x-when");
static H_IDS_ELEMENT: Schema<'static> = Schema::new_member(
    shape_id!("bench", "HeaderIdList", "member"),
    ShapeType::Integer,
    "member",
    0,
);
static H_IDS: Schema<'static> = Schema::new_member(shape_id!("bench", "HeaderInput", "ids"), ShapeType::List, "ids", 3)
    .with_http_header("x-ids")
    .with_list_member(&H_IDS_ELEMENT);
static HEADER_MEMBERS: [&Schema<'static>; 4] = [&H_TOKEN, &H_COUNT, &H_WHEN, &H_IDS];
static HEADER_INPUT: Schema<'static> =
    Schema::new_struct(shape_id!("bench", "HeaderInput"), ShapeType::Structure, &HEADER_MEMBERS)
        .with_http(HttpTrait::new("POST", "/things", Some(200)));

// --- prefix_headers: `@httpPrefixHeaders` map ---
static P_META: Schema<'static> =
    Schema::new_member(shape_id!("bench", "PrefixInput", "meta"), ShapeType::Map, "meta", 0)
        .with_http_prefix_headers("x-meta-");
static PREFIX_MEMBERS: [&Schema<'static>; 1] = [&P_META];
static PREFIX_INPUT: Schema<'static> =
    Schema::new_struct(shape_id!("bench", "PrefixInput"), ShapeType::Structure, &PREFIX_MEMBERS)
        .with_http(HttpTrait::new("POST", "/things", Some(200)));

// --- body_only: three unbound members read through the codec ---
static B_NOTE: Schema<'static> =
    Schema::new_member(shape_id!("bench", "BodyInput", "note"), ShapeType::String, "note", 0);
static B_COUNT: Schema<'static> =
    Schema::new_member(shape_id!("bench", "BodyInput", "count"), ShapeType::Integer, "count", 1);
static B_ENABLED: Schema<'static> = Schema::new_member(
    shape_id!("bench", "BodyInput", "enabled"),
    ShapeType::Boolean,
    "enabled",
    2,
);
static BODY_MEMBERS: [&Schema<'static>; 3] = [&B_NOTE, &B_COUNT, &B_ENABLED];
static BODY_INPUT: Schema<'static> =
    Schema::new_struct(shape_id!("bench", "BodyInput"), ShapeType::Structure, &BODY_MEMBERS)
        .with_original_name("BodyInput")
        .with_http(HttpTrait::new("POST", "/things", Some(200)));

// --- mixed_full_request: every binding location at once ---
static M_NAME: Schema<'static> =
    Schema::new_member(shape_id!("bench", "MixedInput", "name"), ShapeType::String, "name", 0).with_http_label();
static M_AGE: Schema<'static> =
    Schema::new_member(shape_id!("bench", "MixedInput", "age"), ShapeType::Integer, "age", 1).with_http_query("age");
static M_TAGS: Schema<'static> =
    Schema::new_member(shape_id!("bench", "MixedInput", "tags"), ShapeType::List, "tags", 2).with_http_query("tag");
static M_TOKEN: Schema<'static> =
    Schema::new_member(shape_id!("bench", "MixedInput", "token"), ShapeType::String, "token", 3)
        .with_http_header("x-token");
static M_META: Schema<'static> =
    Schema::new_member(shape_id!("bench", "MixedInput", "meta"), ShapeType::Map, "meta", 4)
        .with_http_prefix_headers("x-meta-");
static M_NOTE: Schema<'static> =
    Schema::new_member(shape_id!("bench", "MixedInput", "note"), ShapeType::String, "note", 5);
static MIXED_MEMBERS: [&Schema<'static>; 6] = [&M_NAME, &M_AGE, &M_TAGS, &M_TOKEN, &M_META, &M_NOTE];
static MIXED_INPUT: Schema<'static> =
    Schema::new_struct(shape_id!("bench", "MixedInput"), ShapeType::Structure, &MIXED_MEMBERS)
        .with_original_name("MixedInput")
        .with_http(HttpTrait::new("POST", "/pets/{name}", Some(200)));

// ============================================================================
// Scenarios
// ============================================================================

fn server_request(uri: &str, headers: &[(&str, &str)], body: &'static [u8]) -> ServerRequest {
    let mut builder = http::Request::builder().method("POST").uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request =
        aws_smithy_runtime_api::http::Request::try_from(builder.body(()).unwrap()).expect("valid bench request");
    let parts = request.into_parts();
    ServerRequest {
        uri: parts.uri,
        headers: parts.headers,
        body: Bytes::from_static(body),
    }
}

struct Scenario {
    name: &'static str,
    schema: &'static Schema<'static>,
    request: ServerRequest,
    /// How many members the walker must hand to the consumer; the validate
    /// pass fails loudly if the bench stops exercising what it claims to.
    expected_reads: usize,
}

fn scenarios(codec_content_type: &str, body_only_body: &'static [u8], body_only: bool) -> Vec<Scenario> {
    let body_scenarios = vec![Scenario {
        name: "body_only",
        schema: &BODY_INPUT,
        request: server_request("/things", &[("content-type", codec_content_type)], body_only_body),
        expected_reads: 3,
    }];
    if body_only {
        return body_scenarios;
    }

    let large_query: String = {
        let mut uri = String::from("/things?age=7");
        for i in 0..50 {
            uri.push_str(&format!("&tag=tag-value-{i}"));
        }
        uri
    };
    let large_params: String = {
        let mut uri = String::from("/things?age=7");
        for i in 0..50 {
            // Half the keys repeat, exercising the grouping path.
            uri.push_str(&format!("&key-{}=value-{i}", i % 25));
        }
        uri
    };

    let mut all = vec![
        Scenario {
            name: "label_plain",
            schema: &LABEL_INPUT,
            request: server_request("/pets/rex%20jr", &[], b""),
            expected_reads: 1,
        },
        Scenario {
            name: "label_greedy",
            schema: &GREEDY_INPUT,
            request: server_request("/data/a/b/c/meta", &[], b""),
            expected_reads: 1,
        },
        Scenario {
            name: "query_small",
            schema: &QUERY_INPUT,
            request: server_request("/things?age=7&tag=a&tag=b&tag=c", &[], b""),
            expected_reads: 2,
        },
        Scenario {
            name: "query_large_50_tags",
            schema: &QUERY_INPUT,
            request: server_request(&large_query, &[], b""),
            expected_reads: 2,
        },
        Scenario {
            name: "query_params_map_small",
            schema: &QP_INPUT,
            request: server_request("/things?a=1&b=2&a=3&age=7", &[], b""),
            expected_reads: 2,
        },
        Scenario {
            name: "query_params_map_large_50_pairs",
            schema: &QP_INPUT,
            request: server_request(&large_params, &[], b""),
            expected_reads: 2,
        },
        Scenario {
            name: "headers",
            schema: &HEADER_INPUT,
            request: server_request(
                "/things",
                &[
                    ("x-token", "secret-token-value"),
                    ("x-count", "42"),
                    ("x-when", "Mon, 16 Dec 2019 23:48:18 GMT"),
                    ("x-ids", "1,2,3"),
                    ("x-ids", "4"),
                ],
                b"",
            ),
            expected_reads: 4,
        },
        Scenario {
            name: "mixed_full_request",
            schema: &MIXED_INPUT,
            request: server_request(
                "/pets/rex%20jr?age=7&tag=a&tag=b&tag=c",
                &[
                    ("content-type", codec_content_type),
                    ("x-token", "secret"),
                    ("x-meta-color", "red"),
                    ("x-meta-size", "xl"),
                ],
                br#"{"note":"hello"}"#,
            ),
            expected_reads: 6,
        },
        Scenario {
            name: "prefix_headers",
            schema: &PREFIX_INPUT,
            request: server_request(
                "/things",
                &[
                    ("x-meta-color", "red"),
                    ("x-meta-size", "xl"),
                    ("x-meta-owner", "smithy"),
                    ("x-meta-build", "release"),
                    ("x-meta-region", "us-west-2"),
                    ("x-meta-stage", "prod"),
                    ("x-unrelated", "ignored"),
                    ("user-agent", "bench"),
                ],
                b"",
            ),
            expected_reads: 1,
        },
    ];
    all.extend(body_scenarios);
    all
}

// ============================================================================
// Driving the deserializer
// ============================================================================

/// Reads one bound member the way a generated input walker would, so every
/// scenario pays for actual value parsing, not just routing.
fn drain(member: &Schema<'_>, deserializer: &mut dyn ShapeDeserializer) -> Result<(), SerdeError> {
    match member.shape_type() {
        ShapeType::String => {
            black_box(deserializer.read_string(member)?);
        }
        ShapeType::Integer => {
            black_box(deserializer.read_integer(member)?);
        }
        ShapeType::Boolean => {
            black_box(deserializer.read_boolean(member)?);
        }
        ShapeType::Timestamp => {
            black_box(deserializer.read_timestamp(member)?);
        }
        ShapeType::List => {
            let element_is_integer = member
                .member()
                .map(|element| element.shape_type() == ShapeType::Integer)
                .unwrap_or(false);
            deserializer.read_list(member, &mut |element| {
                if element_is_integer {
                    black_box(element.read_integer(member)?);
                } else {
                    black_box(element.read_string(member)?);
                }
                Ok(())
            })?;
        }
        ShapeType::Map => {
            deserializer.read_map(member, &mut |key, values| {
                black_box(key);
                values.read_list(member, &mut |element| {
                    black_box(element.read_string(member)?);
                    Ok(())
                })
            })?;
        }
        _ => {}
    }
    Ok(())
}

fn run(protocol: &dyn ServerProtocol, scenario: &Scenario) -> usize {
    let mut deserializer = protocol
        .deserialize_request(scenario.schema, &scenario.request)
        .expect("bench request passes the content-type check");
    let mut reads = 0usize;
    deserializer
        .read_struct(scenario.schema, &mut |member, d| {
            reads += 1;
            drain(member, d)
        })
        .expect("bench request deserializes");
    reads
}

fn validate(protocol: &dyn ServerProtocol, scenarios: &[Scenario]) {
    for scenario in scenarios {
        let reads = run(protocol, scenario);
        assert_eq!(
            reads, scenario.expected_reads,
            "scenario `{}` read {} members, expected {}",
            scenario.name, reads, scenario.expected_reads
        );
    }
}

// ============================================================================
// Memory report
// ============================================================================

fn memory_report(protocol_name: &str, protocol: &dyn ServerProtocol, scenarios: &[Scenario]) {
    const ITERATIONS: usize = 1000;
    println!("\nallocations per request ({protocol_name}), averaged over {ITERATIONS} runs:");
    println!("{:<36} {:>12} {:>16}", "scenario", "allocations", "bytes allocated");
    for scenario in scenarios {
        black_box(run(protocol, scenario)); // warm-up outside the measurement
        let allocations_before = ALLOCATIONS.load(Ordering::Relaxed);
        let bytes_before = ALLOCATED_BYTES.load(Ordering::Relaxed);
        for _ in 0..ITERATIONS {
            black_box(run(protocol, scenario));
        }
        let allocations = (ALLOCATIONS.load(Ordering::Relaxed) - allocations_before) / ITERATIONS;
        let bytes = (ALLOCATED_BYTES.load(Ordering::Relaxed) - bytes_before) / ITERATIONS;
        println!("{:<36} {:>12} {:>16}", scenario.name, allocations, bytes);
    }
    println!();
}

// ============================================================================
// Criterion groups
// ============================================================================

fn bench_scenarios(criterion: &mut Criterion, group_name: &str, protocol: &dyn ServerProtocol, scenarios: &[Scenario]) {
    let mut group = criterion.benchmark_group(group_name);
    for scenario in scenarios {
        group.bench_function(scenario.name, |bencher| {
            bencher.iter(|| black_box(run(protocol, black_box(scenario))));
        });
    }
    group.finish();
}

fn request_bindings(criterion: &mut Criterion) {
    let json = RestJson1Protocol::default();
    let json_scenarios = scenarios(
        "application/json",
        br#"{"note":"hello","count":42,"enabled":true}"#,
        false,
    );
    validate(&json, &json_scenarios);
    memory_report("rest_json_1", &json, &json_scenarios);

    // The label/query/header paths never touch the codec, so restXml only
    // runs the codec-delegation baseline.
    let xml = RestXmlProtocol::default();
    let xml_scenarios = scenarios(
        "application/xml",
        b"<BodyInput><note>hello</note><count>42</count><enabled>true</enabled></BodyInput>",
        true,
    );
    validate(&xml, &xml_scenarios);
    memory_report("rest_xml", &xml, &xml_scenarios);

    bench_scenarios(criterion, "rest_json_1", &json, &json_scenarios);
    bench_scenarios(criterion, "rest_xml", &xml, &xml_scenarios);
}

criterion_group!(benches, request_bindings);
criterion_main!(benches);
