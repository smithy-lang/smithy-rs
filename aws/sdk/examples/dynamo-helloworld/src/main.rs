/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::error::Error;


use env_logger::Env;
use dynamodb::{Endpoint, Region, Credentials};
use dynamodb::operation::ListTables;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init_from_env(Env::default().default_filter_or("info"));
    println!("DynamoDB client version: {}", dynamodb::PKG_VERSION);
    let config = dynamodb::Config::builder()
        .region(Region::from("us-east-1"))
        // To load credentials from environment variables, delete this line
        .credentials_provider(Credentials::from_keys(
            "<fill me in2>",
            "<fill me in>",
            None
        ))
        // To use real DynamoDB, delete this line:
        .endpoint_resolver(Endpoint::immutable(http::Uri::from_static(
            "http://localhost:8000",
        )))
        .build();
    let client = aws_hyper::Client::https();

    let op = ListTables::builder().build(&config);
    // Currently this fails, pending the merge of https://github.com/awslabs/smithy-rs/pull/202
    let tables = client.call(op).await?;
    println!("Current DynamoDB tables: {:?}", tables);
    Ok(())
}
