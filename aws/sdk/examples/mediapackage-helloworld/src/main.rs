/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

#[tokio::main]
async fn main() -> Result<(), mediapackage::Error> {
    let client = mediapackage::Client::from_env();
    let list_channels = client.list_channels().send().await?;

    // List out all the mediapackage channels and display their ARN and description.
    if let Some(channels) = list_channels.channels {
        channels.iter().for_each(|c| {
            let description = c.description.to_owned().unwrap_or_default();
            let arn = c.arn.to_owned().unwrap_or_default();

            println!(
                "Channel Description : {}, Channel ARN : {}",
                description, arn
            );
        });
    }

    Ok(())
}
