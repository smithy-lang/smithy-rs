/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_types::region::ProvideRegion;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The AWS Region in which the client is created.
    #[structopt(short, long)]
    default_region: Option<String>,

    /// Whether to display additional runtime information.
    #[structopt(short, long)]
    verbose: bool,
}

/// Lists the Lambda ARNs and runtimes in the given AWS Region.
async fn show_lambdas(reg: String) {
    let r = reg.clone();
    let region = lambda::Region::new(reg);
    let config = lambda::Config::builder().region(region).build();
    let client = lambda::Client::from_conf(config);

    let resp = client.list_functions().send().await;
    println!("Functions in {}:", r);
    println!();

    for function in resp.unwrap().functions.unwrap_or_default() {
        println!("  ARN:     {}", function.function_arn.unwrap());
        println!("  Runtime: {:?}", function.runtime.unwrap());
        println!();
    }
}

/// Lists the ARNs and runtimes of your Lambda functions in all available regions.
/// # Arguments
///
/// * `[-d DEFAULT-REGION]` - The region in which the client is created.
///    If not supplied, uses the value of the **AWS_DEFAULT_REGION** environment variable.
///    If the environment variable is not set, defaults to **us-west-2**.
/// * `[-v]` - Whether to display additional information.
#[tokio::main]
async fn main() -> Result<(), lambda::Error> {
    tracing_subscriber::fmt::init();
    let Opt {
        default_region,
        verbose,
    } = Opt::from_args();

    let region = default_region
        .as_ref()
        .map(|region| ec2::Region::new(region.clone()))
        .or_else(|| aws_types::region::default_provider().region())
        .unwrap_or_else(|| ec2::Region::new("us-west-2"));

    if verbose {
        println!("EC2 client version:    {}", ec2::PKG_VERSION);
        println!("Lambda client version: {}", lambda::PKG_VERSION);
        println!("Region:                {:?}", &region);
    }

    // Get list of available regions.
    let config = ec2::Config::builder().region(region).build();
    let ec2_client = ec2::Client::from_conf(config);
    let resp = ec2_client.describe_regions().send().await;

    for region in resp.unwrap().regions.unwrap_or_default() {
        show_lambdas(region.region_name.unwrap()).await;
    }

    Ok(())
}
