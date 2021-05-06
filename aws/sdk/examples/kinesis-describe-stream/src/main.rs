/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */
use std::process;

use kinesis::{Client, Config, Region};

use aws_types::region::ProvideRegion;

use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The region. Overrides environment variable AWS_DEFAULT_REGION.
    #[structopt(short, long)]
    default_region: Option<String>,

    /// The name of the stream
    #[structopt(short, long)]
    name: String,

    /// Whether to display additional information
    #[structopt(short, long)]
    verbose: bool,
}

/// Describes an Amazon Kinesis data stream.
/// # Arguments
/// * `-n NAME` - The name of the data stream.
/// * `[-d DEFAULT-REGION]` - The region in which the client is created.
///   If not supplied, uses the value of the **AWS_DEFAULT_REGION** environment variable.
///   If the environment variable is not set, defaults to **us-west-2**.
/// * `[-v]` - Whether to display additional information.
#[tokio::main]
async fn main() {
    let Opt {
        default_region,
        name,
        verbose,
    } = Opt::from_args();

    let region = default_region
        .as_ref()
        .map(|region| Region::new(region.clone()))
        .or_else(|| aws_types::region::default_provider().region())
        .unwrap_or_else(|| Region::new("us-west-2"));

    if verbose {
        println!("Kinesis client version: {}\n", kinesis::PKG_VERSION);
        println!("Stream name: {}", name);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let conf = Config::builder().region(region).build();
    let conn = aws_hyper::conn::Standard::https();
    let client = Client::from_conf_conn(conf, conn);

    match client.describe_stream().stream_name(name).send().await {
        Ok(resp) => {
            let desc = resp.stream_description.unwrap();

            println!("Stream description:");
            println!("  Name:              {}:", desc.stream_name.unwrap());
            println!("  Status:            {:?}", desc.stream_status.unwrap());
            println!("  Open shards:       {:?}", desc.shards.unwrap().len());
            println!(
                "  Retention (hours): {}",
                desc.retention_period_hours.unwrap()
            );
            println!("  Encryption:        {:?}", desc.encryption_type.unwrap());
        }
        Err(e) => {
            println!("Got an error describing stream");
            println!("{}", e);
            process::exit(1);
        }
    };
}
