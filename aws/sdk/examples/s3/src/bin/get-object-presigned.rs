/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::input::GetObjectInput;
use aws_sdk_s3::presigning::config::PresigningConfig;
use aws_sdk_s3::{Region, PKG_VERSION};
use std::error::Error;
use std::time::Duration;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The AWS Region.
    #[structopt(short, long)]
    region: Option<String>,

    /// The name of the bucket.
    #[structopt(short, long)]
    bucket: String,

    /// The object key.
    #[structopt(short, long)]
    object: String,

    /// Whether to display additional information.
    #[structopt(short, long)]
    verbose: bool,
}

/// Generates a presigned request for S3 GetObject.
/// # Arguments
///
/// * `[-r REGION]` - The Region in which the client is created.
///   If not supplied, uses the value of the **AWS_REGION** environment variable.
///   If the environment variable is not set, defaults to **us-west-2**.
/// * `[-v]` - Whether to display additional information.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();

    let Opt {
        region,
        bucket,
        object,
        verbose,
    } = Opt::from_args();

    let region_provider = RegionProviderChain::first_try(region.map(Region::new))
        .or_default_provider()
        .or_else(Region::new("us-west-2"));
    let shared_config = aws_config::from_env().region(region_provider).load().await;

    println!();

    if verbose {
        println!("S3 client version: {}", PKG_VERSION);
        println!("Region:            {}", shared_config.region().unwrap());
        println!();
    }

    // TODO(PresignedReqPrototype): Also show an example of the fluent client once it's available
    let presigned_request = GetObjectInput::builder()
        .bucket(bucket)
        .key(object)
        .build()?
        .presigned(
            &shared_config,
            PresigningConfig::expires_in(Duration::from_secs(900))?,
        )
        .await?;

    println!("{:?}", presigned_request);
    Ok(())
}
