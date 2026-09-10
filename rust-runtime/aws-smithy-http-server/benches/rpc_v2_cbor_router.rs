/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Benchmarks the RPC v2 CBOR route parser, router, and routing service.
//!
//! Run with:
//! `cargo bench -p aws-smithy-http-server --bench rpc_v2_cbor_router`
//!
//! `SAMPLE_SIZE` and `MEASUREMENT_TIME_SECS` tune Criterion.

use std::convert::Infallible;
use std::time::Duration;

use aws_smithy_http_server::body::empty;
use aws_smithy_http_server::protocol::rpc_v2_cbor::router::RpcV2CborRouter;
use aws_smithy_http_server::protocol::rpc_v2_cbor::RpcV2Cbor;
use aws_smithy_http_server::routing::{Router, RoutingService};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use http::{HeaderValue, Method, Request, Response};
use tokio::runtime::Runtime;
use tower::{service_fn, ServiceExt};

#[path = "../src/protocol/rpc_v2_cbor/route_identity.rs"]
mod route_identity;

use route_identity::parse_route_identity;

fn parse_route_identity_regex(path: &str) -> Option<route_identity::RouteIdentity<'_>> {
    const IDENTIFIER: &str = r#"((_+([A-Za-z]|[0-9]))|[A-Za-z])[A-Za-z0-9_]*"#;
    static PATH_REGEX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(&format!(
            r#"/service/({IDENTIFIER}\.)*(?P<service>{IDENTIFIER})/operation/(?P<operation>{IDENTIFIER})$"#,
        ))
        .expect("valid legacy route regex")
    });

    let captures = PATH_REGEX.captures(path)?;
    let service = captures.name("service")?;
    let operation = captures.name("operation")?;
    Some(route_identity::RouteIdentity {
        service: service.as_str(),
        operation: operation.as_str(),
        route_key: &path[service.start()..],
    })
}

fn request(path: &str) -> Request<()> {
    Request::builder()
        .method(Method::POST)
        .uri(path)
        .header("smithy-protocol", HeaderValue::from_static("rpc-v2-cbor"))
        .body(())
        .expect("valid benchmark request")
}

fn sample_size() -> usize {
    std::env::var("SAMPLE_SIZE").map_or(100, |value| value.parse().expect("SAMPLE_SIZE must be an integer"))
}

fn measurement_time() -> Duration {
    std::env::var("MEASUREMENT_TIME_SECS").map_or(Duration::from_secs(5), |value| {
        Duration::from_secs(value.parse().expect("MEASUREMENT_TIME_SECS must be an integer"))
    })
}

fn scenarios() -> [(&'static str, Request<()>); 4] {
    [
        ("canonical_hit", request("/service/Service/operation/Operation")),
        (
            "namespaced_hit",
            request("/service/aws.protocoltests.rpcv2Cbor.Service/operation/Operation"),
        ),
        (
            "long_prefix_hit",
            request("/api/2026/tenant/example/service/Service/operation/Operation"),
        ),
        (
            "invalid_suffix_miss",
            request("/service/Service/operation/Operation/invalid-suffix"),
        ),
    ]
}

fn rpc_v2_cbor_router(c: &mut Criterion) {
    let router: RpcV2CborRouter<()> = [("Service/operation/Operation", ())].into_iter().collect();
    let scenarios = scenarios();

    let mut group = c.benchmark_group("rpc_v2_cbor_router/handwritten");
    group.throughput(Throughput::Elements(1));
    for (name, request) in &scenarios {
        group.bench_with_input(BenchmarkId::from_parameter(name), request, |b, request| {
            b.iter(|| black_box(router.match_route(black_box(request))));
        });
    }
    group.finish();
}

fn route_identity_parser(c: &mut Criterion) {
    const URL_COUNT: usize = 16_384;
    // This odd stride visits every entry in the power-of-two corpus while avoiding sequential access.
    const URL_STEP: usize = 8_191;

    type UrlFactory = fn(usize) -> String;
    let scenarios: [(&str, UrlFactory); 4] = [
        ("canonical_hit", |index| {
            format!("/service/Service{index:016X}/operation/Operation{index:016X}")
        }),
        ("namespaced_hit", |index| {
            format!(
                "/service/aws.protocoltests.rpcv2Cbor{index:016X}.Service{index:016X}/operation/Operation{index:016X}"
            )
        }),
        ("long_operation_hit", |index| {
            format!(
                "/service/Service{index:016X}/operation/AnOperationNameThatIsLongEnoughToExposeSuffixScanningCosts{index:016X}"
            )
        }),
        ("invalid_operation_miss", |index| {
            format!("/service/Service{index:016X}/operation/AnOperationNameThatIsAlmostValid{index:016X}-")
        }),
    ];

    type Parser = for<'a> fn(&'a str) -> Option<route_identity::RouteIdentity<'a>>;
    let implementations: [(&str, Parser); 2] = [
        ("legacy_regex", parse_route_identity_regex),
        ("handwritten", parse_route_identity),
    ];

    let mut group = c.benchmark_group("rpc_v2_cbor_route_identity_parser/corpus");
    group.throughput(Throughput::Elements(1));
    for (scenario, make_url) in scenarios {
        // Constructing and allocating the corpus is intentionally outside Criterion's timed loop.
        let urls: Vec<Box<str>> = (0..URL_COUNT).map(|index| make_url(index).into_boxed_str()).collect();

        for (implementation, parser) in implementations {
            let mut index = 0;
            group.bench_function(BenchmarkId::new(implementation, scenario), |b| {
                b.iter(|| {
                    let path = urls[index].as_ref();
                    index += URL_STEP;
                    if index >= URL_COUNT {
                        index -= URL_COUNT;
                    }
                    black_box(parser(black_box(path)))
                });
            });
        }
    }
    group.finish();
}

fn rpc_v2_cbor_routing_service(c: &mut Criterion) {
    let operation = service_fn(|_request: Request<()>| async { Ok::<_, Infallible>(Response::new(empty())) });
    let router: RpcV2CborRouter<_> = [("Service/operation/Operation", operation)].into_iter().collect();
    let service = RoutingService::<_, RpcV2Cbor>::new(router);
    let runtime = Runtime::new().expect("Tokio runtime");

    let mut group = c.benchmark_group("rpc_v2_cbor_routing_service");
    group.throughput(Throughput::Elements(1));
    group.bench_function("handwritten", |b| {
        b.to_async(&runtime).iter(|| {
            let service = service.clone();
            async move {
                black_box(
                    service
                        .oneshot(request("/service/Service/operation/Operation"))
                        .await
                        .expect("infallible operation"),
                )
            }
        });
    });
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(sample_size())
        .measurement_time(measurement_time());
    targets = route_identity_parser, rpc_v2_cbor_router, rpc_v2_cbor_routing_service
}
criterion_main!(benches);
