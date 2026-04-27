/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use clap::Parser;
use std::sync::Arc;
use tokio::time::{Duration, Instant};

#[derive(Parser)]
#[command(about = "Adaptive retry mode test script for SCP validation")]
struct Args {
    /// Number of concurrent workers (async tasks)
    #[arg(long, default_value_t = 1)]
    num_workers: usize,

    /// Test duration in seconds
    #[arg(long)]
    duration: f64,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let client = Arc::new(aws_sdk_dsql::Client::new(&config));
    let duration = Duration::from_secs_f64(args.duration);
    let deadline = Instant::now() + duration;

    let mut handles = Vec::with_capacity(args.num_workers);
    for _ in 0..args.num_workers {
        let client = Arc::clone(&client);
        handles.push(tokio::spawn(async move {
            while Instant::now() < deadline {
                let _ = client.list_clusters().send().await;
            }
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }
}
