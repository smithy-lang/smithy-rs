/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::process;

use kms::Region;

use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[derive(Debug, StructOpt)]
struct Opt {
    /// Specifies the region
    #[structopt(default_value = "us-west-2", short, long)]
    region: String,

    /// The # of bytes (64, 128, or 256)
    #[structopt(short, long)]
    length: String,

    /// The name of the input file with encrypted text to decrypt
    #[structopt(short, long)]
    input: String,

    /// Specifies whether additonal runtime informmation is displayed
    #[structopt(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let mut opt = Opt::from_args();

    match &opt.length[..] {
        "" => opt.length = String::from("256"),
        "64" => opt.length = String::from("64"),
        "128" => opt.length = String::from("128"),
        "256" => opt.length = String::from("256"),
        _ => opt.length = String::from("256"),
    }

    if opt.verbose {
        println!("KMS client version: {}\n", kms::PKG_VERSION);
        println!("Region: {}", opt.region);
        println!("Length: {}", opt.length);
        println!("Input:  {}", opt.input);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let r = &opt.region;
    let l = opt.length.parse::<i32>().unwrap();

    let config = kms::Config::builder()
        .region(Region::new(String::from(r)))
        .build();
    let client = kms::Client::from_conf_conn(config, aws_hyper::conn::Standard::https());

    let resp = match client.generate_random().number_of_bytes(l).send().await {
        Ok(output) => output,
        Err(e) => {
            println!("Caught an error {} calling GenerateRandom", e);
            process::exit(1);
        }
    };

    // Did we get an encrypted blob?
    let blob = resp.plaintext.expect("Could not get encrypted text");
    let bytes = blob.as_ref();

    let s = base64::encode(&bytes);

    println!("\nData key:");
    println!("{}", s);
}
