/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::scenario::{dynamodb, s3, set_keepalive};
use anyhow::bail;
use clap::Parser;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressState, ProgressStyle};
use std::fmt::Write;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

mod scenario;
mod server;

#[derive(clap::Parser, Clone, Copy)]
enum Service {
    DynamoDb,
    S3,
}

impl FromStr for Service {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            s if s.eq_ignore_ascii_case("dynamodb") => Ok(Service::DynamoDb),
            s if s.eq_ignore_ascii_case("s3") => Ok(Service::S3),
            _ => bail!("unknown service {s}"),
        }
    }
}

#[derive(clap::Parser)]
struct Args {
    #[arg(short, long)]
    service: Service,

    #[arg(long)]
    scenario: Option<String>,

    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    if args.verbose {
        tracing_subscriber::fmt::init();
    }
    let mut scenarios = match args.service {
        Service::DynamoDb => vec![
            dynamodb::setup(),
            dynamodb::dynamo_throttling_429(),
            dynamodb::dynamo_throttling_500(),
            dynamodb::dynamo_throttling_503(),
            dynamodb::empty_body_400(),
            dynamodb::dynamo_scenario(500, None),
            dynamodb::dynamo_scenario(503, None),
            dynamodb::dynamo_scenario(500, Some("RequestTimeout")),
            dynamodb::dynamo_scenario(500, Some("InternalServiceError")),
            dynamodb::dynamo_scenario(503, Some("InternalServiceError")),
            set_keepalive(dynamodb::dynamo_scenario(503, Some("InternalServiceError"))),
            dynamodb::throttling_with_close_header(),
            dynamodb::timeout(),
        ],
        Service::S3 => vec![
            s3::setup(),
            s3::s3_scenario(503, Some("ServiceUnavailable")),
            s3::s3_scenario(503, Some("SlowDown")),
            s3::s3_scenario(500, None),
        ],
    };
    if let Some(run_only) = args.scenario {
        scenarios.retain(|scen| scen.name.eq_ignore_ascii_case(&run_only) || scen.name == "setup")
    }
    let progress_bar = ProgressBar::new(scenarios.len() as u64);
    progress_bar.enable_steady_tick(Duration::from_millis(50));
    let spinner_style = ProgressStyle::with_template("{prefix:.bold.dim} {spinner} {wide_msg}")
        .unwrap()
        .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ");
    progress_bar.set_style(spinner_style);
    progress_bar.set_message("waiting for first request...");
    if args.verbose {
        progress_bar.set_draw_target(ProgressDrawTarget::hidden());
    }
    let result = server::start_server(scenarios, Arc::new(progress_bar.clone())).await;
    progress_bar.finish_and_clear();
    println!("Run complete:\n{}", result.unwrap());
}

impl server::Progress for ProgressBar {
    fn scenarios_remaining(&self, scenario: Option<&str>, num_remaining: usize) {
        let scenario = scenario.unwrap_or("done!").to_string();
        self.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} {pos}/{len} [{elapsed_precise}] [{bar:.cyan/blue}] (eta: {eta}) scenario: {scenario}",
            )
            .unwrap()
            .with_key("eta", |state: &ProgressState, w: &mut dyn Write| {
                write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap()
            })
            .with_key("scenario", move |_state: &ProgressState, w: &mut dyn Write| {
                write!(w, "{}", scenario).unwrap()
            })
            .progress_chars("#>-"),
        );
        let total = self.length().unwrap();
        self.set_position(total - num_remaining as u64)
    }
}
