/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::process;

use polly::{Client, Config, Region};

use aws_types::region::{EnvironmentProvider, ProvideRegion};

use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The region
    #[structopt(short, long)]
    region: Option<String>,

    /// Display additional information
    #[structopt(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let Opt {
        region,
        verbose,
    } = Opt::from_args();

    let region = EnvironmentProvider::new()
        .region()
        .or_else(|| region.as_ref().map(|region| Region::new(region.clone())))
        .unwrap_or_else(|| Region::new("us-west-2"));

    if verbose {
        println!("polly client version: {}\n", polly::PKG_VERSION);
        println!("Region: {:?}", &region);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

//    let r = &region;

    let config = Config::builder().region(region).build();

    let client = Client::from_conf_conn(config, aws_hyper::conn::Standard::https());

    match client.describe_voices().send().await {
        Ok(resp) => {
            println!("Voices:");
            let mut l = 0;

            for voice in resp.voices.iter() {
                for v in voice.iter() {
                    l += 1;
                    match &v.name {
                        None => {}
                        Some(x) => println!("  Name:     {}", x),
                    }
                    match &v.language_name {
                        None => {}
                        Some(x) => println!("  Language: {}\n", x),
                    }
                }
            }

            println!("\nFound {} voices\n", l);
        }
        Err(e) => {
            println!("Got an error describing voices:");
            println!("{:?}", e);
            process::exit(1);
        }
    };
}
