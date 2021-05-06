/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::process;

use dynamodb::{Client, Region};

use aws_types::region::ProvideRegion;

use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The region. Overrides environment variable AWS_DEFAULT_REGION.
    #[structopt(short, long)]
    default_region: Option<String>,

    /// The table name
    #[structopt(short, long)]
    table: String,

    /// Activate verbose mode
    #[structopt(short, long)]
    verbose: bool,
}

/// Deletes an Amazon DynamoDB table.
/// # Arguments
/// * `-t TABLE` - The name of the table.
/// * `[-d DEFAULT-REGION]` - The region in which the client is created.
///   If not supplied, uses the value of the **AWS_DEFAULT_REGION** environment variable.
///   If the environment variable is not set, defaults to **us-west-2**.
/// * `[-v]` - Whether to display additional information.
#[tokio::main]
async fn main() {
    let Opt {
        table,
        default_region,
        verbose,
    } = Opt::from_args();

    let region = default_region
        .as_ref()
        .map(|region| Region::new(region.clone()))
        .or_else(|| aws_types::region::default_provider().region())
        .unwrap_or_else(|| Region::new("us-west-2"));

    if verbose {
        println!("DynamoDB client version: {}\n", dynamodb::PKG_VERSION);
        println!("Region: {:?}", &region);
        println!("Table:  {}", table);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let conf = dynamodb::Config::builder().region(region).build();
    let conn = aws_hyper::conn::Standard::https();
    let client = Client::from_conf_conn(conf, conn);

    match client.delete_table().table_name(table).send().await {
        Ok(_) => println!("Deleted table"),
        Err(e) => {
            println!("Got an error deleting the table:");
            println!("{}", e);
            process::exit(1);
        }
    };
}
