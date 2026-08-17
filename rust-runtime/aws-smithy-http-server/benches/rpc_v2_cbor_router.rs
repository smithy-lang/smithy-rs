/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::alloc::{GlobalAlloc, Layout, System};
use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use aws_smithy_http_server::body::empty;
use aws_smithy_http_server::protocol::rpc_v2_cbor::router::benchmarks::NomAllocatingRpcV2CborRouter;
use aws_smithy_http_server::protocol::rpc_v2_cbor::router::RpcV2CborRouter;
use aws_smithy_http_server::protocol::rpc_v2_cbor::RpcV2Cbor;
use aws_smithy_http_server::routing::{Router, RoutingService};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use http::{HeaderValue, Method, Request, Response};
use tokio::runtime::Runtime;
use tower::{service_fn, ServiceExt};

struct CountingAllocator;

static COUNT_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static ALLOCATION_CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: Delegates to the process-wide system allocator with the unchanged layout.
        let pointer = unsafe { System.alloc(layout) };
        record_allocation(pointer, layout.size());
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: Delegates to the process-wide system allocator with the unchanged layout.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        record_allocation(pointer, layout.size());
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: The pointer and layout are passed through to the allocator that created it.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: The pointer, old layout, and new size are passed through unchanged.
        let new_pointer = unsafe { System.realloc(pointer, layout, new_size) };
        record_allocation(new_pointer, new_size);
        new_pointer
    }
}

fn record_allocation(pointer: *mut u8, size: usize) {
    if !pointer.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
        ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(size as u64, Ordering::Relaxed);
    }
}

fn request(path: &str) -> Request<()> {
    Request::builder()
        .method(Method::POST)
        .uri(path)
        .header("smithy-protocol", HeaderValue::from_static("rpc-v2-cbor"))
        .body(())
        .unwrap()
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

fn benchmark_router<R>(c: &mut Criterion, strategy: &str, router: &R, scenarios: &[(&str, Request<()>)])
where
    R: Router<(), Service = ()>,
{
    let mut group = c.benchmark_group(format!("rpc_v2_cbor_router/{strategy}"));
    group.throughput(Throughput::Elements(1));
    for (name, request) in scenarios {
        group.bench_with_input(BenchmarkId::from_parameter(name), request, |b, request| {
            b.iter(|| black_box(router.match_route(black_box(request))));
        });
    }
    group.finish();
}

fn allocation_totals<R>(router: &R, request: &Request<()>, iterations: u64) -> (u64, u64)
where
    R: Router<(), Service = ()>,
{
    ALLOCATION_CALLS.store(0, Ordering::Relaxed);
    ALLOCATED_BYTES.store(0, Ordering::Relaxed);
    COUNT_ALLOCATIONS.store(true, Ordering::Relaxed);
    for _ in 0..iterations {
        black_box(router.match_route(black_box(request))).ok();
    }
    COUNT_ALLOCATIONS.store(false, Ordering::Relaxed);
    (
        ALLOCATION_CALLS.load(Ordering::Relaxed),
        ALLOCATED_BYTES.load(Ordering::Relaxed),
    )
}

fn report_allocations<R>(strategy: &str, router: &R, scenarios: &[(&str, Request<()>)])
where
    R: Router<(), Service = ()>,
{
    const ITERATIONS: u64 = 10_000;
    eprintln!("allocation report: {strategy}");
    for (name, request) in scenarios {
        let (calls, bytes) = allocation_totals(router, request, ITERATIONS);
        eprintln!(
            "  {name}: {:.2} allocations/route, {:.2} bytes/route",
            calls as f64 / ITERATIONS as f64,
            bytes as f64 / ITERATIONS as f64,
        );
    }
}

fn rpc_v2_cbor_router(c: &mut Criterion) {
    let nom_allocating: NomAllocatingRpcV2CborRouter<()> = [("Service.Operation", ())].into_iter().collect();
    let borrowed_path: RpcV2CborRouter<()> = [("Service/operation/Operation", ())].into_iter().collect();
    let scenarios = scenarios();

    report_allocations("nom_allocating", &nom_allocating, &scenarios);
    report_allocations("borrowed_path", &borrowed_path, &scenarios);
    benchmark_router(c, "nom_allocating", &nom_allocating, &scenarios);
    benchmark_router(c, "borrowed_path", &borrowed_path, &scenarios);
}

fn rpc_v2_cbor_routing_service(c: &mut Criterion) {
    let operation = service_fn(|_request: Request<()>| async { Ok::<_, Infallible>(Response::new(empty())) });
    let nom_router: NomAllocatingRpcV2CborRouter<_> = [("Service.Operation", operation.clone())].into_iter().collect();
    let borrowed_router: RpcV2CborRouter<_> = [("Service/operation/Operation", operation)].into_iter().collect();
    let nom_service = RoutingService::<_, RpcV2Cbor>::new(nom_router);
    let borrowed_service = RoutingService::<_, RpcV2Cbor>::new(borrowed_router);
    let runtime = Runtime::new().expect("Tokio runtime");

    let mut group = c.benchmark_group("rpc_v2_cbor_routing_service");
    group.throughput(Throughput::Elements(1));
    group.bench_function("nom_allocating", |b| {
        b.to_async(&runtime).iter(|| {
            let service = nom_service.clone();
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
    group.bench_function("borrowed_path", |b| {
        b.to_async(&runtime).iter(|| {
            let service = borrowed_service.clone();
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
    targets = rpc_v2_cbor_router, rpc_v2_cbor_routing_service
}
criterion_main!(benches);
