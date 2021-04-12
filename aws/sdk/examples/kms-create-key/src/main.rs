/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::process;

use kms::Region;

use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(default_value = "us-west-2", short, long)]
    region: String,

    /// Activate verbose mode    
    #[structopt(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
   let opt = Opt::from_args();

    if opt.verbose {
        println!("KMS client version: {}\n", kms::PKG_VERSION);
        println!("Region: {}", opt.region);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let r = &opt.region;

    let config = kms::Config::builder()
        .region(Region::new(String::from(r)))
        .build();

    let client = kms::Client::from_conf_conn(config, aws_hyper::conn::Standard::https());

    match client.create_key().send().await {
        Ok(data) => match data.key_metadata {
            None => println!("No metadata found"),
            Some(x) => match x.key_id {
                None => println!("No key id"),
                Some(k) => println!("\n\nKey:\n{}", k),
            },
        },
        Err(_) => {
            println!("");
            process::exit(1);
        }
    };
    println!("");
}
