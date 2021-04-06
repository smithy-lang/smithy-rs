/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use clap::{App, Arg};

use std::env;
use std::fs;
use std::process;

use aws_hyper::SdkError;
use kms::error::{DecryptError, DecryptErrorKind};
use kms::Blob;
use kms::Client;
use kms::Region;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

async fn display_error_hint(client: &Client, err: DecryptError) {
    eprintln!("Error while decrypting: {}", err);
    match err.kind {
        DecryptErrorKind::NotFoundError(_) => {
            let existing_keys = client
                .list_keys()
                .send()
                .await
                .expect("failure to list keys");
            let existing_keys = existing_keys
                .keys
                .unwrap_or_default()
                .into_iter()
                .map(|key| key.key_id.expect("keys must have ids"))
                .collect::<Vec<_>>();
            eprintln!(
                "  hint: Did you create the key first?\n  Existing keys in this region: {:?}",
                existing_keys
            )
        }
        _ => (),
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
    let key = matches.value_of("key").expect("required");
    let text = matches.value_of("text").expect("required");
    let verbose = matches.is_present("verbose");

    if verbose {
        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let config = kms::Config::builder().region(Region::from(region)).build();

    let client = kms::Client::from_conf_conn(config, aws_hyper::conn::Standard::https());

    // Open text file and get contents as a string
    // input is a base-64 encoded string, so decode it:
    let data = fs::read_to_string(text)
        .map(|text| base64::decode(text).expect("invalid base 64"))
        .map(Blob::new);

    let resp = match client
        .decrypt()
        .key_id(key)
        .ciphertext_blob(data.unwrap())
        .send()
        .await
    {
        Ok(output) => output,
        Err(SdkError::ServiceError { err, .. }) => {
            display_error_hint(&client, err).await;
            process::exit(1);
        }
        Err(other) => {
            eprintln!("Encryption failure: {}", other);
            process::exit(1);
        }
    };

    let inner = resp.plaintext.unwrap();
    let bytes = inner.as_ref();

    let s = match String::from_utf8(bytes.to_vec()) {
        Ok(v) => v,
        Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
    };

    println!("");
    println!("Decoded string:");
    println!("{}", s);
}
