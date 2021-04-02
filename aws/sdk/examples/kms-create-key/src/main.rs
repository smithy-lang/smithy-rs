/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use clap::{App, Arg};

use kms::operation::CreateKey;
use kms::Region;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[tokio::main]
async fn main() {
    let matches = App::new("myapp")
        .arg(
            Arg::with_name("region")
                .short("r")
                .long("region")
                .value_name("REGION")
                .help("Specifies the region")
                .takes_value(true),
        )
        .get_matches();

    let region = matches.value_of("region").unwrap_or("us-west-2");

    println!("Region: {}", region);

    SubscriberBuilder::default()
        .with_env_filter("info")
        .with_span_events(FmtSpan::CLOSE)
        .init();
    let config = kms::Config::builder().region(Region::from(region)).build();

    let client = aws_hyper::Client::https();

    let data = client
        .call(CreateKey::builder().build(&config))
        .await
        .expect("failed to create key");

    println!("");

    match data.key_metadata {
        None => println!("No metadata found"),
        Some(x) => match x.key_id {
            None => println!("No key id"),
            Some(k) => println!("{}", k),
        },
    }
}
