// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
mod events;
mod metrics;
use bench_sdk::{Client, Config};
use hdrhistogram::Histogram;
use serde_json::json;
use std::io::{BufRead, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Semaphore};

#[derive(Clone)]
struct Work {
    direction: String,
    kind: String,
    size: usize,
    count: usize,
    preflight: bool,
}
struct Samples {
    streams: u64,
    events: u64,
    first: Histogram<u64>,
    completion: Histogram<u64>,
    roundtrip: Histogram<u64>,
}
impl Samples {
    fn new() -> Self {
        Self {
            streams: 0,
            events: 0,
            first: Histogram::new(3).unwrap(),
            completion: Histogram::new(3).unwrap(),
            roundtrip: Histogram::new(3).unwrap(),
        }
    }
    fn merge(&mut self, other: Self) {
        self.streams += other.streams;
        self.events += other.events;
        self.first.add(other.first).unwrap();
        self.completion.add(other.completion).unwrap();
        self.roundtrip.add(other.roundtrip).unwrap();
    }
    fn record(h: &mut Histogram<u64>, elapsed: Duration) {
        h.record(elapsed.as_nanos().max(1) as u64).unwrap();
    }
}

async fn stream(client: &Client, w: &Work) -> Samples {
    let mut samples = Samples::new();
    let start = Instant::now();
    let count = w.count;
    let (stamps, mut receive_stamps) = mpsc::channel::<Instant>(64);
    // In preflight, the producer cannot finish the request until the first echo arrives.
    let gate = Arc::new(Semaphore::new(0));
    let sender_gate = gate.clone();
    let work = w.clone();
    let source = async_stream::stream! {
        for i in 0..count {
            let value = events::event(&work.kind, work.size, i);
            if work.direction == "duplex" {
                stamps.send(Instant::now()).await.unwrap();
            }
            yield Ok(value);
            if work.preflight && work.direction == "duplex" && i == 0 {
                sender_gate.acquire().await.unwrap().forget();
            }
        }
    };
    match w.direction.as_str() {
        "upload" => {
            let out = client
                .upload()
                .kind(&w.kind)
                .size(w.size as i32)
                .count(count as i32)
                .events(source.into())
                .send()
                .await
                .unwrap();
            assert_eq!(out.count, count as i32);
            assert_eq!(out.metadata, events::METADATA);
        }
        "download" => {
            let mut out = client
                .download()
                .kind(&w.kind)
                .size(w.size as i32)
                .count(count as i32)
                .fail(false)
                .send()
                .await
                .unwrap();
            assert_eq!(out.metadata, events::METADATA);
            for i in 0..count {
                let v = out.events.recv().await.unwrap().expect("complete download");
                events::verify(&v, &w.kind, w.size, i);
                if i == 0 {
                    Samples::record(&mut samples.first, start.elapsed());
                }
            }
            assert!(
                out.events.recv().await.unwrap().is_none(),
                "extra download event"
            );
        }
        "duplex" => {
            let mut out = client
                .duplex()
                .kind(&w.kind)
                .size(w.size as i32)
                .count(count as i32)
                .events(source.into())
                .send()
                .await
                .unwrap();
            assert_eq!(out.metadata, events::METADATA);
            for i in 0..count {
                let v = out.events.recv().await.unwrap().expect("complete echo");
                let sent = receive_stamps.recv().await.unwrap();
                Samples::record(&mut samples.roundtrip, sent.elapsed());
                events::verify(&v, &w.kind, w.size, i);
                if i == 0 {
                    Samples::record(&mut samples.first, start.elapsed());
                    gate.add_permits(1);
                }
            }
            assert!(
                out.events.recv().await.unwrap().is_none(),
                "extra echo event"
            );
        }
        _ => panic!("unknown direction"),
    }
    Samples::record(&mut samples.completion, start.elapsed());
    samples.streams = 1;
    samples.events = count as u64; // duplex = completed event round trips
    samples
}

async fn phase(client: Client, work: Work, concurrency: usize, duration: Duration) -> Samples {
    let end = Instant::now() + duration;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..concurrency {
        let client = client.clone();
        let work = work.clone();
        tasks.spawn(async move {
            let mut out = Samples::new();
            while Instant::now() < end {
                out.merge(stream(&client, &work).await);
            }
            out
        });
    }
    let mut out = Samples::new();
    while let Some(s) = tasks.join_next().await {
        out.merge(s.unwrap());
    }
    out
}

fn distribution(h: &Histogram<u64>) -> serde_json::Value {
    if h.is_empty() {
        return serde_json::Value::Null;
    }
    json!({"samples": h.len(), "p50_ns": h.value_at_quantile(0.5), "p95_ns": h.value_at_quantile(0.95), "p99_ns": h.value_at_quantile(0.99)})
}

#[allow(deprecated)]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    assert_eq!(
        a.len(),
        10,
        "endpoint direction kind size count concurrency warmup seconds preflight"
    );
    let work = Work {
        direction: a[2].clone(),
        kind: a[3].clone(),
        size: a[4].parse().unwrap(),
        count: a[5].parse().unwrap(),
        preflight: a[9] == "true",
    };
    assert!(work.count >= 2 && work.size > 0);
    let concurrency: usize = a[6].parse().unwrap();
    let warmup = Duration::from_secs_f64(a[7].parse().unwrap());
    let duration = Duration::from_secs_f64(a[8].parse().unwrap());
    let workers: usize = std::env::var("BENCH_CLIENT_WORKERS")
        .unwrap_or_else(|_| "2".into())
        .parse()
        .unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut http = hyper014::Client::builder();
        http.http2_only(true);
        let transport = aws_smithy_http_client::hyper_014::HyperClientBuilder::new()
            .hyper_builder(http).build(hyper014::client::HttpConnector::new());
        let config = Config::builder().behavior_version(bench_sdk::config::BehaviorVersion::latest()).endpoint_url(&a[1]).http_client(transport)
            .retry_config(bench_sdk::config::retry::RetryConfig::disabled()).build();
        let client = Client::from_conf(config);
        if work.preflight {
            // Check modeled stream error after ordinary payload/order/EOS checks.
            stream(&client, &work).await;
            let mut out = client.download().kind(&work.kind).size(work.size as i32).count(2).fail(true).send().await.unwrap();
            assert_eq!(out.metadata, events::METADATA);
            for i in 0..2 { events::verify(&out.events.recv().await.unwrap().unwrap(), &work.kind, work.size, i); }
            let error = out.events.recv().await.expect_err("modeled stream failure");
            assert!(error.as_service_error().expect("modeled service error").is_stream_failure());
            assert!(format!("{error:?}").contains("expected stream failure"));
            println!("{}", json!({"preflight": "passed", "duplex_before_request_eos": work.direction == "duplex"}));
            return;
        }
        phase(client.clone(), work.clone(), concurrency, warmup).await;
        println!("WARM");
        std::io::stdout().flush().unwrap();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "go");
        metrics::reset_peak();
        let before = metrics::snapshot();
        let start = Instant::now();
        let samples = phase(client, work.clone(), concurrency, duration).await;
        let elapsed = start.elapsed().as_secs_f64();
        let after = metrics::snapshot();
        assert!(samples.events > 0);
        println!("{}", json!({
            "elapsed_seconds": elapsed, "streams": samples.streams, "events": samples.events,
            "events_per_second": samples.events as f64 / elapsed,
            "application_mib_per_second": samples.events as f64 * work.size as f64 / (1048576.0 * elapsed),
            "application_bytes_per_event": work.size,
            "application_bytes_definition": "text or blob bytes only; duplex counts one completed round trip",
            "completion": distribution(&samples.completion), "first_event": distribution(&samples.first),
            "roundtrip": distribution(&samples.roundtrip), "before": before, "after": after
        }));
        // Keep the process alive until the controller has flushed per-PID profiles.
        std::io::stdout().flush().unwrap();
        line.clear();
        std::io::stdin().lock().read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "done");
    });
}
