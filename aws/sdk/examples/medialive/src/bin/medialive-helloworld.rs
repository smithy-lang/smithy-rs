/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_types::region::{self, ProvideRegion};
use medialive::{Client, Config, Error, Region, PKG_VERSION};
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

/// Lists your AWS Elemental MediaLive input names and ARNs in the Region.
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

    let region = region::ChainProvider::first_try(default_region.map(Region::new))
        .or_default_provider()
        .or_else(Region::new("us-west-2"));

    println!();

    if verbose {
        println!("MediaLive version: {}", PKG_VERSION);
        println!("Region:            {:?}", region.region().await);
        println!();
    }

    let conf = Config::builder().region(region).build().await;
    let client = Client::from_conf(conf);

    let input_list = client.list_inputs().send().await?;

    for i in input_list.inputs.unwrap_or_default() {
        let input_arn = i.arn.as_deref().unwrap_or_default();
        let input_name = i.name.as_deref().unwrap_or_default();

        println!("Input Name : {}, Input ARN : {}", input_name, input_arn);
    }

    Ok(())
}
