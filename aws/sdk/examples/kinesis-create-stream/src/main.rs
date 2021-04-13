/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use kinesis::{Client, Config, Region};

use aws_types::region::{EnvironmentProvider, ProvideRegion};
use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(short, long)]
    region: Option<String>,

    #[structopt(short, long)]
    name: String,

    #[structopt(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let Opt {
        name,
        region,
        verbose,
    } = Opt::from_args();
    let region = EnvironmentProvider::new()
        .region()
        .or_else(|| region.as_ref().map(|region| Region::new(region.clone())))
        .unwrap_or_else(|| Region::new("us-east-1"));

    if verbose {
        println!("kinesis client version: {}\n", kinesis::PKG_VERSION);
        println!("Region:      {:?}", &region);
        println!("Stream name: {}", &name);
        // print any other opt settings

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let config = Config::builder().region(&region).build();

    let client = Client::from_conf_conn(config, aws_hyper::conn::Standard::https());

    match client.create_stream().stream_name(&name).send().await {
        Ok(_) => println!("\nCreated stream {} in {:?} region.\n", &name, &region),
        Err(e) => {
            println!("Got an error creating stream name {}:", name);
            println!("{}", e);
        }
    };
}
