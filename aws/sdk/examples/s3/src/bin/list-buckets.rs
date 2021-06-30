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

    /// Whether to display additional information.
    #[structopt(short, long)]
    verbose: bool,
}

/// Lists your Amazon S3 buckets
/// # Arguments
///
/// * `[-d DEFAULT-REGION]` - The Region in which the client is created.
///   If not supplied, uses the value of the **AWS_REGION** environment variable.
///   If the environment variable is not set, defaults to **us-west-2**.
/// * `[-v]` - Whether to display additional information.
#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt::init();

    let Opt {
        default_region,
        verbose,
    } = Opt::from_args();

    let region = default_region
        .as_ref()
        .map(|region| Region::new(region.clone()))
        .or_else(|| aws_types::region::default_provider().region())
        .unwrap_or_else(|| Region::new("us-west-2"));

    println!();

    if verbose {
        println!("S3 version: {}", PKG_VERSION);
        println!("Region:     {:?}", &region);
        println!();
    }

    let config = Config::builder().region(&region).build();
    let client = Client::from_conf(config);

    let mut num_buckets = 0;

    let resp = client.list_buckets().send().await?;
    let buckets = resp.buckets.unwrap_or_default();

    for bucket in &buckets {
        println!("{}", bucket.name.as_deref().unwrap_or_default());
        num_buckets += 1;
    }

    println!();
    println!("Found {} buckets", num_buckets);

    Ok(())
}
