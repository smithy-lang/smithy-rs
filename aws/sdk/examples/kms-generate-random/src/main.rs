/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::process;

use kms::{Client, Config, Region};

use aws_types::region::{EnvironmentProvider, ProvideRegion};

use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The region
    #[structopt(short, long)]
    region: Option<String>,

    /// The # of bytes (64, 128, or 256)
    #[structopt(short, long)]
    length: String,

    /// Specifies whether additonal runtime informmation is displayed
    #[structopt(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let Opt {
        mut length,
        region,
        verbose,
    } = Opt::from_args();

    let region = EnvironmentProvider::new()
        .region()
        .or_else(|| region.as_ref().map(|region| Region::new(region.clone())))
        .unwrap_or_else(|| Region::new("us-west-2"));

    match &length[..] {
        "" => length = String::from("256"),
        "64" => length = String::from("64"),
        "128" => length = String::from("128"),
        "256" => length = String::from("256"),
        _ => length = String::from("256"),
    }

    if verbose {
        println!("KMS client version: {}\n", kms::PKG_VERSION);
        println!("Region: {:?}", &region);
        println!("Length: {}", length);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let l = length.parse::<i32>().unwrap();

    let config = Config::builder().region(region).build();
    let client = Client::from_conf(config);

    let resp = match client.generate_random().number_of_bytes(l).send().await {
        Ok(output) => output,
        Err(e) => {
            println!("Caught an error {} calling GenerateRandom", e);
            process::exit(1);
        }
    };

    // Did we get an encrypted blob?
    let blob = resp.plaintext.expect("Could not get encrypted text");
    let bytes = blob.as_ref();

    let s = base64::encode(&bytes);

    println!("\nData key:");
    println!("{}", s);
}
