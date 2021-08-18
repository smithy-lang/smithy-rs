/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_types::region::{ChainProvider, ProvideRegion, Region};
use kms::{Client, Config, Error, PKG_VERSION};
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
/// Creates an AWS KMS key.
/// # Arguments
///
/// * `[-d DEFAULT-REGION]` - The Region in which the client is created.
///    If not supplied, uses the value of the **AWS_REGION** environment variable.
///    If the environment variable is not set, defaults to **us-west-2**.
/// * `[-v]` - Whether to display additional information.
#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt::init();

    let Opt {
        default_region,
        verbose,
    } = Opt::from_args();

    let region = ChainProvider::first_try(default_region.map(Region::new))
        .or_default_provider()
        .or_else(Region::new("us-west-2"));

    println!();

    if verbose {
        println!("KMS version: {}", PKG_VERSION);
        println!("Region:      {:?}", region.region().await);
        println!();
    }

    let conf = Config::builder().region(region).build().await;
    let client = Client::from_conf(conf);

    let resp = client.create_key().send().await?;

    let id = resp
        .key_metadata
        .unwrap()
        .key_id
        .unwrap_or_else(|| String::from("No ID!"));

    println!("Key: {}", id);

    Ok(())
}
