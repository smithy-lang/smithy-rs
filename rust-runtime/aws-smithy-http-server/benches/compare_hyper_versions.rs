/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Benchmark comparing aws-smithy-legacy-http-server (hyper 0.14) vs aws-smithy-http-server (hyper 1.x)
//!
//! This benchmark uses Unix sockets for fast, isolated testing of connection handling
//! performance without network stack overhead.
//!
//! Run with:
//!   cargo bench --bench compare_hyper_versions
//!
//! Adjust parameters with environment variables:
//!   CONCURRENCY=200 REQUESTS=20000 DURATION=60 cargo bench --bench compare_hyper_versions

use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};

use http::{Request, Response};
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::{Bytes, Incoming};
use hyper_util::client::legacy::Client;
use tokio::net::UnixListener;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tower::service_fn;

const DEFAULT_CONCURRENCY: usize = 100;
const DEFAULT_TOTAL_REQUESTS: usize = 10_000;
const DEFAULT_DURATION_SECS: u64 = 30;

struct BenchmarkResults {
    total_requests: usize,
    duration: Duration,
    requests_per_sec: f64,
    avg_latency_micros: f64,
    success_count: usize,
    error_count: usize,
}

impl BenchmarkResults {
    fn print(&self, _server_name: &str) {
        println!("  Total requests:     {}", self.total_requests);
        println!("  Duration:           {:.2}s", self.duration.as_secs_f64());
        println!("  Requests/sec:       {:.2}", self.requests_per_sec);
        println!("  Avg latency:        {:.2}µs", self.avg_latency_micros);
        println!("  Success:            {}", self.success_count);
        println!("  Errors:             {}", self.error_count);
        println!();
    }
}

/// Simple handler that returns "Hello, World!"
async fn handle_request(_req: Request<Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(Response::new(Full::new(Bytes::from("Hello, World!"))))
}

/// Start the new server (aws-smithy-http-server with hyper 1.x)
async fn start_new_server(socket_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Remove existing socket file
    let _ = std::fs::remove_file(socket_path);

    let listener = UnixListener::bind(socket_path)?;
    let service = service_fn(handle_request);

    aws_smithy_http_server::serve(
        listener,
        aws_smithy_http_server::routing::IntoMakeService::new(service),
    )
    .await?;

    Ok(())
}

/// Start the legacy server (aws-smithy-legacy-http-server with hyper 0.14)
async fn start_legacy_server(socket_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use hyper014::service::service_fn as hyper_service_fn;

    // Remove existing socket file
    let _ = std::fs::remove_file(socket_path);

    let listener = UnixListener::bind(socket_path)?;

    // Manual accept loop for Unix sockets with hyper 0.14
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                tokio::spawn(async move {
                    if let Err(err) = hyper014::server::conn::Http::new()
                        .serve_connection(
                            stream,
                            hyper_service_fn(|_req: hyper014::Request<hyper014::Body>| async {
                                Ok::<_, Infallible>(hyper014::Response::new(hyper014::Body::from(
                                    "Hello, World!",
                                )))
                            }),
                        )
                        .await
                    {
                        eprintln!("Error serving connection: {}", err);
                    }
                });
            }
            Err(err) => {
                eprintln!("Error accepting connection: {}", err);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

/// Run load test with concurrent requests
async fn load_test(
    socket_path: &str,
    concurrency: usize,
    total_requests: usize,
    duration_secs: u64,
) -> BenchmarkResults {
    // Wait for server to be ready
    tokio::time::sleep(Duration::from_millis(100)).await;

    let start = Instant::now();
    let max_duration = Duration::from_secs(duration_secs);

    let semaphore = Arc::new(Semaphore::new(concurrency));
    let mut tasks = JoinSet::new();

    let success_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let error_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let total_latency_micros = Arc::new(std::sync::atomic::AtomicU64::new(0));

    for _ in 0..total_requests {
        // Check if we've exceeded max duration
        if start.elapsed() >= max_duration {
            break;
        }

        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let socket_path = socket_path.to_string();
        let success_count = success_count.clone();
        let error_count = error_count.clone();
        let total_latency_micros = total_latency_micros.clone();

        tasks.spawn(async move {
            let _permit = permit; // Hold permit until task completes

            let req_start = Instant::now();

            let connector: Client<hyperlocal::UnixConnector, Empty<Bytes>> =
                <Client<hyperlocal::UnixConnector, Empty<Bytes>> as hyperlocal::UnixClientExt<Empty<Bytes>>>::unix();
            let url = hyperlocal::Uri::new(&socket_path, "/");

            match connector.get(url.into()).await {
                Ok(resp) => {
                    // Consume the body to ensure full request completion
                    let _ = resp.into_body().collect().await;

                    let latency = req_start.elapsed().as_micros() as u64;
                    total_latency_micros.fetch_add(latency, std::sync::atomic::Ordering::Relaxed);
                    success_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                Err(_) => {
                    error_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        });

        // Prevent spawning too many tasks at once
        while tasks.len() >= concurrency * 2 {
            let _ = tasks.join_next().await;
        }
    }

    // Wait for all tasks to complete
    while let Some(_) = tasks.join_next().await {}

    let duration = start.elapsed();
    let success = success_count.load(std::sync::atomic::Ordering::Relaxed);
    let errors = error_count.load(std::sync::atomic::Ordering::Relaxed);
    let total_latency = total_latency_micros.load(std::sync::atomic::Ordering::Relaxed);

    let requests_completed = success + errors;
    let requests_per_sec = requests_completed as f64 / duration.as_secs_f64();
    let avg_latency_micros = if success > 0 {
        total_latency as f64 / success as f64
    } else {
        0.0
    };

    BenchmarkResults {
        total_requests: requests_completed,
        duration,
        requests_per_sec,
        avg_latency_micros,
        success_count: success,
        error_count: errors,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read configuration from environment variables
    let concurrency = std::env::var("CONCURRENCY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_CONCURRENCY);

    let total_requests = std::env::var("REQUESTS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_TOTAL_REQUESTS);

    let duration_secs = std::env::var("DURATION")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_DURATION_SECS);

    let legacy_socket = "/tmp/bench_legacy_server.sock";
    let new_socket = "/tmp/bench_new_server.sock";

    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║     Hyper Version Comparison Benchmark                    ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();
    println!("Configuration:");
    println!("  Concurrency:        {}", concurrency);
    println!("  Total requests:     {}", total_requests);
    println!("  Max duration:       {}s", duration_secs);
    println!("  Transport:          Unix sockets");
    println!();
    println!("════════════════════════════════════════════════════════════");
    println!();

    // Benchmark Legacy Server (hyper 0.14)
    println!("🔷 Testing Legacy Server (hyper 0.14)");
    println!("────────────────────────────────────────────────────────────");

    // Start legacy server in background
    let legacy_socket_clone = legacy_socket.to_string();
    let legacy_server = tokio::spawn(async move {
        if let Err(e) = start_legacy_server(&legacy_socket_clone).await {
            eprintln!("Legacy server error: {}", e);
        }
    });

    // Run load test
    let legacy_results = load_test(legacy_socket, concurrency, total_requests, duration_secs).await;
    legacy_results.print("Legacy");

    // Stop legacy server
    legacy_server.abort();
    let _ = std::fs::remove_file(legacy_socket);

    // Brief pause between tests
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Benchmark New Server (hyper 1.x)
    println!("🔶 Testing New Server (hyper 1.x)");
    println!("────────────────────────────────────────────────────────────");

    // Start new server in background
    let new_socket_clone = new_socket.to_string();
    let new_server = tokio::spawn(async move {
        if let Err(e) = start_new_server(&new_socket_clone).await {
            eprintln!("New server error: {}", e);
        }
    });

    // Run load test
    let new_results = load_test(new_socket, concurrency, total_requests, duration_secs).await;
    new_results.print("New");

    // Stop new server
    new_server.abort();
    let _ = std::fs::remove_file(new_socket);

    // Print comparison
    println!("════════════════════════════════════════════════════════════");
    println!();
    println!("📊 Summary");
    println!("────────────────────────────────────────────────────────────");
    println!();

    let speedup = new_results.requests_per_sec / legacy_results.requests_per_sec;
    let latency_improvement = (legacy_results.avg_latency_micros - new_results.avg_latency_micros)
        / legacy_results.avg_latency_micros * 100.0;

    println!("  Legacy (hyper 0.14):   {:.2} req/s", legacy_results.requests_per_sec);
    println!("  New (hyper 1.x):       {:.2} req/s", new_results.requests_per_sec);
    println!();
    println!("  Throughput change:     {:.2}% ({:.2}x)", (speedup - 1.0) * 100.0, speedup);
    println!("  Latency improvement:   {:.2}%", latency_improvement);
    println!();

    if speedup > 1.01 {
        println!("  ✅ New server is faster!");
    } else if speedup < 0.99 {
        println!("  ⚠️  Legacy server was faster");
    } else {
        println!("  ≈  Performance is similar");
    }
    println!();
    println!("════════════════════════════════════════════════════════════");

    Ok(())
}
