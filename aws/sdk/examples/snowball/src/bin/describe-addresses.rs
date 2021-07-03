use aws_sdk_snowball::{Config, Region};

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

#[tokio::main]
async fn main() -> Result<(), aws_sdk_snowball::Error> {
    let region = Region::new("us-east-1");
    let conf = Config::builder().region(region).build();
    let client = aws_sdk_snowball::Client::from_conf(conf);
    let addresses = client.describe_addresses().send().await?;
    for address in addresses.addresses.unwrap() {
        println!("Address: {:?}", address);
    }

    Ok(())
}
