/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use clap::{App, Arg};

use std::env;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::process;

use aws_hyper::SdkError;
use kms::error::{ReEncryptError, ReEncryptErrorKind};
use kms::Blob;
use kms::Client;
use kms::Region;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

async fn display_error_hint(client: &Client, err: ReEncryptError) {
    eprintln!("Error while decrypting: {}", err);
    match err.kind {
        ReEncryptErrorKind::NotFoundError(_) => {
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
            Arg::with_name("source")
                .short("s")
                .long("source")
                .value_name("SOURCE")
                .help("Specifies the source (original) encryption key")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("destination")
                .short("d")
                .long("destination")
                .value_name("DESTINATION")
                .help("Specifies the destination (new) encryption key")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("input")
                .short("i")
                .long("input")
                .value_name("INPUT")
                .help("Specifies the name of the input file containing the text to reencrypt")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("output")
                .short("o")
                .long("output")
                .value_name("OUTPUT")
                .help("Specifies the name of the output file to store the reencrypted text in.")
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
    let source = matches.value_of("source").expect("required");
    let destination = matches.value_of("destination").expect("required");
    let input = matches.value_of("input").unwrap_or("input.txt");
    let output = matches.value_of("output").unwrap_or("output.txt");
    let verbose = matches.is_present("verbose");

    if verbose {
        println!("Running ReEncryptData with args:");
        println!("Region:             {}", region);
        println!("Source key ID:      {}", source);
        println!("Destination key ID: {}", destination);
        println!("Input filename:     {}", input);
        println!("Output filename:    {}", output);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let config = kms::Config::builder().region(Region::from(region)).build();
    let client = kms::Client::from_conf_conn(config, aws_hyper::conn::Standard::https());

    // Get blob from input file
    // Open input text file and get contents as a string
    // input is a base-64 encoded string, so decode it:
    let data = fs::read_to_string(input)
        .map(|input| base64::decode(input).expect("invalid base 64"))
        .map(Blob::new);

    let resp = match client
        .re_encrypt()
        .ciphertext_blob(data.unwrap())
        .source_key_id(source)
        .destination_key_id(destination)
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

    // Did we get an encrypted blob?
    let blob = resp.ciphertext_blob.expect("Could not get encrypted text");
    let bytes = blob.as_ref();

    let s = base64::encode(&bytes);

    let mut ofile = File::create(output).expect("unable to create file");
    ofile.write_all(s.as_bytes()).expect("unable to write");

    if verbose {
        println!("Wrote the following to {}:", output);
        println!("{}", s);
    } else {
        println!("Wrote base64-encoded output to {}", output);
    }
}
