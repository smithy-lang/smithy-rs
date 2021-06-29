/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_types::region::ProvideRegion;
use lambda::model::Runtime;
use lambda::{Client, Config, Error, Region, PKG_VERSION};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The default AWS Region.
    #[structopt(short, long)]
    default_region: Option<String>,

    /// The Lambda function's ARN.
    #[structopt(short, long)]
    arn: String,

    /// Whether to display additional runtime information.
    #[structopt(short, long)]
    verbose: bool,
}

/// Sets a Lambda function's Java runtime to Corretto.
/// # Arguments
///
/// * `-a ARN` - The ARN of the Lambda function.
/// * `[-d DEFAULT-REGION]` - The Region in which the client is created.
///    If not supplied, uses the value of the **AWS_REGION** environment variable.
///    If the environment variable is not set, defaults to **us-west-2**.
/// * `[-v]` - Whether to display additional information.
#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt::init();

    let Opt {
        arn,
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
        println!("Lambda version:      {}", PKG_VERSION);
        println!("Region:              {:?}", &region);
        println!("Lambda function ARN: {}", &arn);
        println!();
    }

    let config = Config::builder().region(region).build();
    let client = Client::from_conf(config);

    // Get function's runtime
    let resp = client
        .list_functions()
        .send()
        .await
        .expect("Could not get list of functions");

    for function in resp.functions.unwrap_or_default() {
        if arn == function.function_arn.unwrap() {
            let rt = function.runtime.unwrap();
            if rt == Runtime::Java11 || rt == Runtime::Java8 {
                // Change it to Java8a12 (Corretto)
                println!("Original runtime: {:?}", rt);
                let result = client
                    .update_function_configuration()
                    .function_name(function.function_name.unwrap())
                    .runtime(Runtime::Java8al2)
                    .send()
                    .await;

                let result_rt = result.unwrap().runtime.unwrap();
                println!("New runtime: {:?}", result_rt);
            }
        }
    }

    Ok(())
}
