/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::process;

use lambda::{error::InvokeErrorKind, Client, Config, Region, SdkError};

use aws_types::region::ProvideRegion;

use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[tokio::main]
async fn main() {
    let region = aws_types::region::default_provider()
        .region()
        .unwrap_or_else(|| Region::new("us-west-2"));

    println!("Lambda client version: {}", lambda::PKG_VERSION);
    println!("Region:      {:?}", &region);

    SubscriberBuilder::default()
        .with_env_filter("info")
        .with_span_events(FmtSpan::CLOSE)
        .init();

    let config = Config::builder().region(region).build();

    let client = Client::from_conf(config);

    match client
        .invoke()
        .function_name("arn:aws:lambda:us-west-2:892717189312:function:my-rusty-func")
        .send()
        .await
    {
        Ok(resp) => {
            println!("Response:");
            println!("  {:?}", resp.payload);
        }
        Err(SdkError::ServiceError { err, .. })
            if matches!(err.kind, InvokeErrorKind::ResourceNotFoundError(_)) =>
        {
            println!("This lambda function does not exist");
            process::exit(1);
        }
        Err(err) => {
            println!("Got an error listing functions:");
            println!("{}", err);
            process::exit(1);
        }
    };
}
