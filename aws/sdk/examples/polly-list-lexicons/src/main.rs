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

    /// Activate verbose mode    
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
        println!("Region:      {:?}", &region);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

//    let r = &opt.region;

    let config = Config::builder().region(region).build();

    let client = Client::from_conf_conn(config, aws_hyper::conn::Standard::https());

    match client.list_lexicons().send().await {
        Ok(resp) => {
            println!("Lexicons:");
            let mut l = 0;

            for lexicon in resp.lexicons.iter() {
                for lex in lexicon.iter() {
                    l += 1;
                    match &lex.name {
                        None => {}
                        Some(x) => println!("  Name:     {}", x),
                    }
                    match &lex.attributes {
                        None => {}
                        Some(x) => println!("  Language: {:?}\n", x.language_code),
                    }
                }
            }

            println!("\nFound {} lexicons.\n", l);
        }
        Err(e) => {
            println!("Got an error listing lexicons:");
            println!("{:?}", e);
            process::exit(1);
        }
    };
}
