/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::process;

use polly::{Client, Config, Region};

use aws_types::region::ProvideRegion;

use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The region. Overrides environment variable AWS_DEFAULT_REGION.
    #[structopt(short, long)]
    default_region: Option<String>,

    /// Display additional information
    #[structopt(short, long)]
    verbose: bool,
}

/// Lists the voices that are available for use when requesting speech synthesis.
/// # Arguments
///
/// * `[-d DEFAULT-REGION]` - The region in which the client is created.
///    If not supplied, uses the value of the **AWS_DEFAULT_REGION** environment variable.
///    If the environment variable is not set, defaults to **us-west-2**.
/// * `[-v]` - Whether to display additional information.
#[tokio::main]
async fn main() {
    let Opt {
        default_region,
        verbose,
    } = Opt::from_args();

    let region = default_region
        .as_ref()
        .map(|region| Region::new(region.clone()))
        .or_else(|| aws_types::region::default_provider().region())
        .unwrap_or_else(|| Region::new("us-west-2"));

    if verbose {
        println!("polly client version: {}\n", polly::PKG_VERSION);
        println!("Region: {:?}", &region);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let conf = Config::builder().region(region).build();
    let conn = aws_hyper::conn::Standard::https();
    let client = Client::from_conf_conn(conf, conn);

    match client.describe_voices().send().await {
        Ok(resp) => {
            println!("Voices:");
            let voices = resp.voices.unwrap_or_default();
            for voice in &voices {
                println!(
                    "  Name:     {}",
                    voice.name.as_deref().unwrap_or("No name!")
                );
                println!(
                    "  Language:     {}",
                    voice.language_name.as_deref().unwrap_or("No language!")
                );
            }

            println!("\nFound {} voices\n", voices.len());
        }
        Err(e) => {
            println!("Got an error describing voices:");
            println!("{}", e);
            process::exit(1);
        }
    };
}
