/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::scenario::dynamodb::*;
use crate::scenario::s3;
use anyhow::bail;
use clap::Parser;
use std::str::FromStr;

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
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    tracing_subscriber::fmt::init();
    let scenarios = match args.service {
        Service::DynamoDb => vec![
            setup(),
            dynamo_throttling_429(),
            dynamo_throttling_500(),
            dynamo_throttling_503(),
            empty_body_400(),
            dynamo_scenario(500, None),
            dynamo_scenario(503, None),
            dynamo_scenario(500, Some("RequestTimeout")),
            throttling_with_close_header(),
        ],
        Service::S3 => vec![
            s3::setup(),
            s3::s3_scenario(503, Some("ServiceUnavailable")),
            s3::s3_scenario(503, Some("SlowDown")),
            s3::s3_scenario(500, None),
        ],
    };

    let result = server::start_server(scenarios).await;
    println!("Run complete:\n{}", result.unwrap());
}
