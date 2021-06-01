/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

#[tokio::main]
async fn main() -> Result<(), mediapackage::Error> {
    let client = mediapackage::Client::from_env();
    let or_endpoints = client.list_origin_endpoints().send().await?;

    match or_endpoints.origin_endpoints {
        Some(endpoints) => endpoints.iter().for_each(|e| {
            let endpoint_url = e.url.to_owned().unwrap_or_default();
            let endpoint_description = e.description.to_owned().unwrap_or_default();
            println!(
                "Endpoint Description: {}, Endpoint URL : {}",
                endpoint_description, endpoint_url
            )
        }),
        None => println!("No Endpoints found."),
    }

    Ok(())
}
