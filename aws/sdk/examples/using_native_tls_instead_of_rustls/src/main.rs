/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

/// The SDK defaults to using RusTLS by default but you can also use [`native_tls`](https://github.com/sfackler/rust-native-tls)
/// which will choose a TLS implementation appropriate for your platform.
#[tokio::main]
async fn main() -> Result<(), aws_sdk_s3::Error> {
    tracing_subscriber::fmt::init();

    let shared_config = aws_config::load_from_env().await;

    let s3_config = aws_sdk_s3::Config::from(&shared_config);
    let client = aws_sdk_s3::Client::from_conf(s3_config);

    let resp = client.list_buckets().send().await?;

    for bucket in resp.buckets().unwrap_or_default() {
        println!("bucket: {:?}", bucket.name().unwrap_or_default())
    }

    Ok(())
}
