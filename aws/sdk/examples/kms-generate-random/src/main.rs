/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use clap::{App, Arg};

use std::env;
use std::process;

use kms::Region;

use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

fn v_print(v: bool, s: &str) {
    if v {
        println!("{}", s);
    }
}

#[tokio::main]
async fn main() {
    let matches = App::new("myapp")
        .arg(
            Arg::with_name("region")
                .short("r")
                .long("region")
                .value_name("REGION")
                .help("Specifies the region")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("length")
                .short("l")
                .long("length")
                .value_name("LENGTH")
                .help("The # of bytes (64, 128, or 256).")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .value_name("VERBOSE")
                .help("Whether to display additional runtime information.")
                .takes_value(false),
        )
        .get_matches();

    // Get value of AWS_DEFAULT_REGION, if set.
    let default_region = match env::var("AWS_DEFAULT_REGION") {
        Ok(val) => val,
        Err(_e) => "us-west-2".to_string(),
    };

    let region = matches.value_of("region").unwrap_or(&*default_region);
    let length = matches.value_of("length").unwrap_or("");
    let verbose = matches.is_present("verbose");

    let mut l = length.parse::<i32>().unwrap();

    match l {
        0 => {
            v_print(verbose, "Length was zero, setting to 256");
            l = 256;
        }
        64 => v_print(verbose, "Length is 64"),
        128 => v_print(verbose, "Length is 128"),
        _ => {
            if verbose {
                println!("Length was {}, setting it to 256", length);
                l = 256;
            }
        }
    }

    if verbose {
        println!("\nGenerateRandom called with options:");
        println!("  Region:           {}", region);
        println!("  Length (# bytes): {}\n", length);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let config = kms::Config::builder().region(Region::from(region)).build();
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
