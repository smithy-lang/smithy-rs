/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::fs::File;
use std::io::Write;
use std::process;

use aws_hyper::SdkError;

use kms::error::{EncryptError, EncryptErrorKind};
use kms::{Blob, Client, Region};

use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

async fn display_error_hint(client: &Client, err: EncryptError) {
    eprintln!("Error while decrypting: {}", err);
    match err.kind {
        EncryptErrorKind::NotFoundError(_) => {
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

#[derive(Debug, StructOpt)]
struct Opt {
    /// Specifies the region
    #[structopt(default_value = "us-west-2", short, long)]
    region: String,

    /// Specifies the encryption key
    #[structopt(short, long)]
    key: String,

    /// Specifies the text to encrypt
    #[structopt(short, long)]
    text: String,

    /// Specifies the name of the file to store the encrypted text in
    #[structopt(short, long)]
    out: String,

    /// Whether to display additional runtime information
    #[structopt(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();

    if opt.verbose {
        println!("KMS client version: {}\n", kms::PKG_VERSION);
        println!("Region: {}", opt.region);
        println!("Key:    {}", opt.key);
        println!("Text:   {}", opt.text);
        println!("Out:     {}", opt.out);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let r = &opt.region;
    let o = &opt.out;

    let config = kms::Config::builder()
        .region(Region::new(String::from(r)))
        .build();

    let client = kms::Client::from_conf_conn(config, aws_hyper::conn::Standard::https());

    let blob = Blob::new(opt.text.as_bytes());

    let resp = match client
        .encrypt()
        .key_id(opt.key)
        .plaintext(blob)
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

    let mut ofile = File::create(o).expect("unable to create file");
    ofile.write_all(s.as_bytes()).expect("unable to write");

    println!("Wrote the following to {}", o);
    println!("{}", s);
}
