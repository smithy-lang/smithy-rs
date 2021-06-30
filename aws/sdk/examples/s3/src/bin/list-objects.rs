/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_types::region::ProvideRegion;
use s3::{Client, Config, Error, Region, PKG_VERSION};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The default AWS Region.
    #[structopt(short, long)]
    default_region: Option<String>,

    /// The name of the bucket.
    #[structopt(short, long)]
    bucket: String,

    /// Whether to display additional information.
    #[structopt(short, long)]
    verbose: bool,
}

/// Lists the objects in an Amazon S3 bucket.
/// # Arguments
///
/// * `-n NAME` - The name of the bucket.
/// * `[-d DEFAULT-REGION]` - The Region in which the client is created.
///   If not supplied, uses the value of the **AWS_REGION** environment variable.
///   If the environment variable is not set, defaults to **us-west-2**.
/// * `[-v]` - Whether to display additional information.
#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt::init();

    let Opt {
        default_region,
        bucket,
        verbose,
    } = Opt::from_args();

    let region = default_region
        .as_ref()
        .map(|region| Region::new(region.clone()))
        .or_else(|| aws_types::region::default_provider().region())
        .unwrap_or_else(|| Region::new("us-west-2"));

    if verbose {
        println!("S3 version: {}", PKG_VERSION);
        println!("Region:     {:?}", &region);
        println!();
    }

    let config = Config::builder().region(&region).build();
    let client = Client::from_conf(config);

    println!();

    let resp = client.list_objects().bucket(&bucket).send().await?;

    for object in resp.contents.unwrap_or_default() {
        println!(" {}", object.key.as_deref().unwrap_or_default());
    }

    Ok(())
}
