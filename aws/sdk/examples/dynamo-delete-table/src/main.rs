/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::process;

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

    /// Activate verbose mode    
    #[structopt(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();

    if opt.table == "" {
        println!("\nYou must supply a table name");
        println!("-t TABLE)\n");
        process::exit(1);
    }

    if opt.verbose {
        println!("DynamoDB client version: {}\n", dynamodb::PKG_VERSION);
        println!("Region: {}", opt.region);
        println!("Table:  {}", opt.table);
        //    println!("Key:   {}\n", opt.key);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let t = &opt.table;
    let r = &opt.region;

    let config = dynamodb::Config::builder()
        .region(Region::new(String::from(r)))
        .build();

    let client = dynamodb::Client::from_conf_conn(config, aws_hyper::conn::Standard::https());

    match client
        .delete_table()
        .table_name(String::from(t))
        .send()
        .await
    {
        Ok(_) => println!("Deleted table {} in region {}", opt.table, opt.region),
        Err(e) => {
            println!("Got an error deleting the table:");
            println!("{:?}", e);
            process::exit(1);
        }
    };
}
