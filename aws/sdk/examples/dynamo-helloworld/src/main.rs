/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::error::Error;


use env_logger::Env;
use smithy_http::endpoint::Endpoint;
use aws_types::region::Region;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init_from_env(Env::default().default_filter_or("info"));
    let _config = dynamodb::Config::builder()
        .region(Region::from("us-east-1"))
        // To load credentials from environment variables, delete this line
        .credentials_provider(aws_auth::Credentials::from_keys(
            "<fill me in2>",
            "<fill me in>",
            None
        ))
        // To use real DynamoDB, delete this line:
        .endpoint_resolver(Endpoint::immutable(http::Uri::from_static(
            "http://localhost:8000",
        )))
        .build();
    Ok(())
    // WIP: Pending merge of `aws-hyper` PR
}
