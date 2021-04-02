/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use clap::{App, Arg};

use std::fs;
use std::process;
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
                .takes_value(true),
        )
        .arg(
            Arg::with_name("text")
                .short("t")
                .long("text")
                .value_name("TEXT")
                .help("Specifies file with encrypted text to decrypt")
                .takes_value(true),
        )
        .get_matches();

    let region = matches.value_of("region").unwrap_or("us-west-2");
    let key = matches.value_of("key").unwrap_or("");
    let text = matches.value_of("text").unwrap_or("");

    if region == "" || key == "" || text == "" {
        println!("You must supply a value for region, key, and file containing encrypted text ([-r REGION] -k KEY -t TEXT-FILE)");
        println!("If REGION is omitted, defaults to us-west-2");

        process::exit(1);
    }

    SubscriberBuilder::default()
        .with_env_filter("info")
        .with_span_events(FmtSpan::CLOSE)
        .init();
    let config = kms::Config::builder().region(Region::from(region)).build();

    let client = aws_hyper::Client::https();

    // Vector to hold the string parts/u8 values
    let mut v_bytes: Vec<u8> = Vec::new();

    // Open text file and get contents as a string
    match fs::read_to_string(text) {
        Ok(input) => {
            // Split the string into parts by comma
            let parts = input.split(",");
            for s in parts {
                // Trim any trailing line feed
                let mut my_string = String::from(s);

                let len = my_string.trim_end_matches(&['\r', '\n'][..]).len();
                my_string.truncate(len);

                match my_string.parse::<u8>() {
                    Ok(num) => v_bytes.push(num),
                    Err(e) => {
                        println!("Got an error parsing '{}'", s);
                        panic!("{}", e)
                    }
                }
            }
        }
        Err(_) => println!("Could not parse {} as a string", text),
    }

    let blob = Blob::new(v_bytes);

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
