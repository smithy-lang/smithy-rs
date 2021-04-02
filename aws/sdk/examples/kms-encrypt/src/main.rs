/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use clap::{App, Arg};

use std::process;

use kms::operation::Encrypt;
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
                .help("Specifies the text to encrypt")
                .takes_value(true),
        )
        .get_matches();

    let region = matches.value_of("region").unwrap_or("us-west-2");
    let key = matches.value_of("key").unwrap_or("");
    let text = matches.value_of("text").unwrap_or("");

    if region == "" || key == "" || text == "" {
        println!(
            "You must supply a value for region, key, and text ([-r REGION] -k KEY -t \"TEXT\")"
        );
        println!("If REGION is not supplied, defaults to us-west-2");

        process::exit(1);
    }

    SubscriberBuilder::default()
        .with_env_filter("info")
        .with_span_events(FmtSpan::CLOSE)
        .init();
    let config = kms::Config::builder().region(Region::from(region)).build();

    let client = aws_hyper::Client::https();

    let blob = Blob::new(text.as_bytes());

    let resp = client
        .call(
            Encrypt::builder()
                .key_id(key)
                .plaintext(blob)
                .build(&config),
        )
        .await
        .expect("failed to encrypt text");

    // Did we get an encrypted blob?
    let blob = resp.ciphertext_blob.expect("Could not get encrypted text");
    let bytes = blob.as_ref();
    let len = bytes.len();
    let mut i = 0;

    for b in bytes {
        if i < len - 1 {
            print!("{},", b);
        } else {
            // Don't add a comma after the last item
            println!("{}", b);
        }
        i += 1;
    }

    println!("");
}
