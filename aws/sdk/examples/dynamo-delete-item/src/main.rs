/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::process;

use dynamodb::model::AttributeValue;
use dynamodb::Region;

use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(default_value = "us-west-2", short, long)]
    region: String,

    /// The table name
    #[structopt(short, long)]
    table: String,

    /// The key for the item in the table
    #[structopt(short, long)]
    key: String,

    /// The value of the item to delete from the table
    #[structopt(short, long)]
    value: String,

    /// Activate info mode
    #[structopt(short, long)]
    info: bool,
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();

    if opt.table == "" || opt.key == "" {
        println!("\nYou must supply a table name and key");
        println!("-t TABLE -k KEY)\n");
        process::exit(1);
    }

    if opt.info {
        println!("DynamoDB client version: {}\n", dynamodb::PKG_VERSION);
        println!("Region: {}", opt.region);
        println!("Table:  {}", opt.table);
        println!("Key:   {}\n", opt.key);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let t = &opt.table;
    let k = &opt.key;
    let v = &opt.value;
    let r = &opt.region;

    let config = dynamodb::Config::builder()
        .region(Region::new(String::from(r)))
        .build();

    let client = dynamodb::Client::from_conf_conn(config, aws_hyper::conn::Standard::https());

    match client
        .delete_item()
        .table_name(String::from(t))
        .key(String::from(k), AttributeValue::S(String::from(v)))
        .send()
        .await
    {
        Ok(_) => println!(
            "Deleted {} from table {} in region {}",
            opt.value, opt.table, opt.region
        ),
        Err(e) => {
            println!("Got an error creating table:");
            println!("{:?}", e);
            process::exit(1);
        }
    };
}
