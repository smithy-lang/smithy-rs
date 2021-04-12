/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::fs;
use std::process;

use aws_hyper::SdkError;

use kms::error::{DecryptError, DecryptErrorKind};
use kms::{Blob, Client, Region};

use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[derive(Debug, StructOpt)]
struct Opt {
    /// Specifies the region
    #[structopt(default_value = "us-west-2", short, long)]
    region: String,

    /// Specifies the encryption key
    #[structopt(short, long)]
    key: String,

    /// The name of the input file with encrypted text to decrypt
    #[structopt(short, long)]
    input: String,

    /// Specifies whether to display additonal runtime informmation
    #[structopt(short, long)]
    verbose: bool,
}

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
    let opt = Opt::from_args();

    if opt.verbose {
        println!("KMS client version: {}\n", kms::PKG_VERSION);
        println!("Region: {}", opt.region);
        println!("Key:    {}", opt.key);
        println!("Input:  {}", opt.input);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let r = &opt.region;

    let config = kms::Config::builder()
        .region(Region::new(String::from(r)))
        .build();
    let client = kms::Client::from_conf_conn(config, aws_hyper::conn::Standard::https());

    // Open input text file and get contents as a string
    // input is a base-64 encoded string, so decode it:
    let data = fs::read_to_string(opt.input)
        .map(|input| base64::decode(input).expect("invalid base 64"))
        .map(Blob::new);

    let resp = match client
        .decrypt()
        .key_id(opt.key)
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
