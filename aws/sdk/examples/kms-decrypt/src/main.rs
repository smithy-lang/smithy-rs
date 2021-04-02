/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use clap::{App, Arg};

extern crate base64;

use std::env;
use std::fs;
use std::str;

use kms::operation::Decrypt;
use kms::Blob;
use kms::Region;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

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
            Arg::with_name("key")
                .short("k")
                .long("key")
                .value_name("KEY")
                .help("Specifies the encryption key")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("text")
                .short("t")
                .long("text")
                .value_name("TEXT")
                .help("Specifies file with encrypted text to decrypt")
                .takes_value(true)
                .required(true),
        )
        .get_matches();

    // Get value of AWS_DEFAULT_REGION, if set.
    let default_region;
    match env::var("AWS_DEFAULT_REGION") {
        Ok(val) => default_region = val,
        Err(_e) => default_region = "us-west-2".to_string(),
    }

    let region = matches.value_of("region").unwrap_or(&*default_region);
    let key = matches.value_of("key").unwrap_or("");
    let text = matches.value_of("text").unwrap_or("");

    SubscriberBuilder::default()
        .with_env_filter("info")
        .with_span_events(FmtSpan::CLOSE)
        .init();
    let config = kms::Config::builder().region(Region::from(region)).build();

    let client = aws_hyper::Client::https();

    // Vector to hold the string once we decode it from base64.
    let mut my_bytes: Vec<u8> = Vec::new();

    // Open text file and get contents as a string
    match fs::read_to_string(text) {
        Ok(input) => {
            // input is a base-64 encoded string, so decode it:
            my_bytes = base64::decode(input).unwrap();
        }
        Err(_) => println!("Could not parse {} as a string", text),
    }

    let blob = Blob::new(my_bytes);

    let resp = client
        .call(
            Decrypt::builder()
                .key_id(key)
                .ciphertext_blob(blob)
                .build(&config),
        )
        .await
        .expect("failed to decrypt text");

    let inner = resp.plaintext.unwrap();

    let bytes = inner.as_ref();

    let s = match str::from_utf8(&bytes) {
        Ok(v) => v,
        Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
    };

    println!("");
    println!("Decoded string:");
    println!("{}", s);
}
