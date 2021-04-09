/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::process;

use dynamodb::Region;

use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(default_value = "us-west-2", short, long)]
    region: String,

    #[structopt(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();

    if opt.verbose {
        println!("DynamoDB client version: {}\n", dynamodb::PKG_VERSION);
        println!("Region: {}", opt.region);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let r = &opt.region;

    let config = dynamodb::Config::builder()
        .region(Region::new(String::from(r)))
        .build();

    let client = dynamodb::Client::from_conf_conn(config, aws_hyper::conn::Standard::https());

    match client.list_tables().send().await {
        Ok(resp) => {
            println!("Tables in {}", opt.region);
            let mut l = 0;

            for name in resp.table_names.iter() {
                for n in name.iter() {
                    l = l + 1;
                    println!("    {:?}", n);
                }
            }

            println!("\nFound {} tables in {} region.\n", l, opt.region);
        }
        Err(e) => {
            println!("Got an error listing tables:");
            println!("{:?}", e);
            process::exit(1);
        }
    };
}
