/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::process;

use dynamodb::model::{
    AttributeDefinition, KeySchemaElement, KeyType, ProvisionedThroughput, ScalarAttributeType,
};
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

    /// The region
    #[structopt(short, long)]
    key: String,

    /// Activate verbose mode    
    #[structopt(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();

    if opt.table == "" || opt.key == "" {
        println!("\nYou must supply a table name and key");
        println!("-t TABLE -k KEY)\n");
        process::exit(1);
    }

    if opt.verbose {
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
    let r = &opt.region;

    let config = dynamodb::Config::builder()
        .region(Region::new(String::from(r)))
        .build();

    let client = dynamodb::Client::from_conf_conn(config, aws_hyper::conn::Standard::https());

    let ad = AttributeDefinition::builder()
        .attribute_name(String::from(k))
        .attribute_type(ScalarAttributeType::S)
        .build();

    let ks = KeySchemaElement::builder()
        .attribute_name(String::from(k))
        .key_type(KeyType::Hash)
        .build();

    let pt = ProvisionedThroughput::builder()
        .read_capacity_units(10)
        .write_capacity_units(5)
        .build();

    match client
        .create_table()
        .table_name(String::from(t))
        .key_schema(ks)
        .attribute_definitions(ad)
        .provisioned_throughput(pt)
        .send()
        .await
    {
        Ok(_) => println!(
            "Added table {} with key {} in region {}",
            opt.table, opt.key, opt.region
        ),
        Err(e) => {
            println!("Got an error creating table:");
            println!("{:?}", e);
            process::exit(1);
        }
    };
}
