/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::input::PutObjectInput;
use aws_sdk_s3::presigning::{config::PresigningConfig, request::PresignedRequest};
use aws_sdk_s3::{Client, Config, Region, PKG_VERSION};
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

    /// How long in seconds before the presigned request should expire.
    #[structopt(short, long)]
    expires_in: Option<u64>,

    /// Whether to display additional information.
    #[structopt(short, long)]
    verbose: bool,
}

/// Generates a presigned request for S3 PutObject.
/// # Arguments
///
/// * `[-r REGION]` - The Region in which the client is created.
///   If not supplied, uses the value of the **AWS_REGION** environment variable.
///   If the environment variable is not set, defaults to **us-west-2**.
/// * `[-e EXPIRES_IN]` - The amount of time the presigned request should be valid for.
///   If not given, this defaults to 15 minutes.
/// * `[-v]` - Whether to display additional information.
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();

    let Opt {
        region,
        bucket,
        object,
        expires_in,
        verbose,
    } = Opt::from_args();
    let expires_in = Duration::from_secs(expires_in.unwrap_or(900));

    let region_provider = RegionProviderChain::first_try(region.map(Region::new))
        .or_default_provider()
        .or_else(Region::new("us-west-2"));
    let shared_config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&shared_config);

    println!();

    if verbose {
        println!("S3 client version: {}", PKG_VERSION);
        println!("Region:            {}", shared_config.region().unwrap());
        println!();
    }

    // Presigned requests can be made with the client directly
    let presigned_request = client
        .put_object()
        .bucket(&bucket)
        .key(&object)
        .metadata("some-metadata", "abcd")
        .presigned(PresigningConfig::expires_in(expires_in)?)
        .await?;
    println!("From client: {:?}\n", presigned_request.uri());

    // Or, they can be made directly from an operation input
    let presigned_request = PutObjectInput::builder()
        .bucket(bucket)
        .key(object)
        .metadata("some-metadata", "abcd")
        .build()?
        .presigned(
            &Config::from(&shared_config),
            PresigningConfig::expires_in(expires_in)?,
        )
        .await?;
    println!("From operation input: {:?}\n", presigned_request.uri());

    print_as_curl_request(&presigned_request, Some("some test content"));

    Ok(())
}

fn print_as_curl_request(presigned_req: &PresignedRequest, body: Option<&str>) {
    println!(
        "curl -X {} {} \\",
        presigned_req.method(),
        presigned_req.uri()
    );

    if let Some(body) = body {
        println!("-d '{}' \\", body);
    }

    for (name, value) in presigned_req.headers() {
        // This value conversion method is naïve and will drop values that aren't valid UTF8
        // It's only here for demonstration purposes; Don't use this unless you're confident
        // that your header values are valid UTF-8
        println!(
            "-H '{}: {}' \\",
            name,
            value.to_str().unwrap_or_default().to_string()
        )
    }

    println!("--verbose");
}
