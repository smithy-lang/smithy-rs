/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use http::Uri;
use iam::Endpoint;

#[tokio::main]
async fn main() -> Result<(), iam::Error> {
    tracing_subscriber::fmt::init();
    let ep = Endpoint::immutable(Uri::from_static("https://iam.amazonaws.com"));
    let conf = iam::Config::builder().endpoint_resolver(ep).build();
    let client = iam::Client::from_conf(conf);
    let rsp = client.list_policies().send().await?;
    for policy in rsp.policies.unwrap_or_default() {
        println!(
            "arn: {}; description: {}",
            policy.arn.unwrap(),
            policy.description.unwrap_or_default()
        );
    }
    Ok(())
}
