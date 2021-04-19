/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::process;

use dynamodb::model::AttributeValue;
use dynamodb::{Client, Config, Region};

use aws_types::region::{EnvironmentProvider, ProvideRegion};

use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The region
    #[structopt(short, long)]
    region: Option<String>,

    /// The table name
    #[structopt(short, long)]
    table: String,

    /// The key for the item in the table
    #[structopt(short, long)]
    key: String,

    /// The value of the item to delete from the table
    #[structopt(short, long)]
    value: String,

    /// Whether to display additional information
    #[structopt(short, long)]
    info: bool,
}

#[tokio::main]
async fn main() {
    let Opt {
        info,
        key,
        region,
        table,
        value,
    } = Opt::from_args();

    let region = EnvironmentProvider::new()
        .region()
        .or_else(|| region.as_ref().map(|region| Region::new(region.clone())))
        .unwrap_or_else(|| Region::new("us-west-2"));

    if info {
        println!("DynamoDB client version: {}\n", dynamodb::PKG_VERSION);
        println!("Region: {:?}", &region);
        println!("Table:  {}", table);
        println!("Key:    {}\n", key);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let config = Config::builder().region(region).build();

    let client = Client::from_conf_conn(config, aws_hyper::conn::Standard::https());

    match client
        .delete_item()
        .table_name(table)
        .key(key, AttributeValue::S(value))
        .send()
        .await
    {
        Ok(_) => println!("Deleted item from table"),
        Err(e) => {
            println!("Got an error creating table:");
            println!("{:?}", e);
            process::exit(1);
        }
    };
}
