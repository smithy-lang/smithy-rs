/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

#[tokio::main]
async fn main() -> Result<(), iam::Error> {
    tracing_subscriber::fmt::init();
    let client = iam::Client::from_env();
    let rsp = client.list_access_keys().send().await?;
    for akid in rsp.access_key_metadata.unwrap_or_default() {
        println!(
            "akid: {}; status: {:?}",
            akid.access_key_id.unwrap(),
            akid.status
        );
    }
    Ok(())
}
