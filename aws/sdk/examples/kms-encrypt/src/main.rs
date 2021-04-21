/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::fs::File;
use std::io::Write;
use std::process;

use kms::{Blob, Client, Config, Region};

use aws_types::region::{EnvironmentProvider, ProvideRegion};

use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The region
    #[structopt(short, long)]
    region: Option<String>,

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
    let Opt {
        key,
        out,
        region,
        text,
        verbose,
    } = Opt::from_args();

    let region = EnvironmentProvider::new()
        .region()
        .or_else(|| region.as_ref().map(|region| Region::new(region.clone())))
        .unwrap_or_else(|| Region::new("us-west-2"));

    if verbose {
        println!("KMS client version: {}\n", kms::PKG_VERSION);
        println!("Region: {:?}", &region);
        println!("Key:    {}", key);
        println!("Text:   {}", text);
        println!("Out:    {}", out);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let config = Config::builder().region(region).build();

    let client = Client::from_conf(config);

    let blob = Blob::new(text.as_bytes());

    let resp = match client.encrypt().key_id(key).plaintext(blob).send().await {
        Ok(output) => output,
        Err(e) => {
            eprintln!("Encryption failure: {}", e);
            process::exit(1);
        }
    };

    // Did we get an encrypted blob?
    let blob = resp.ciphertext_blob.expect("Could not get encrypted text");
    let bytes = blob.as_ref();

    let s = base64::encode(&bytes);

    let mut ofile = File::create(&out).expect("unable to create file");
    ofile.write_all(s.as_bytes()).expect("unable to write");

    if verbose {
        println!("Wrote the following to {}", &out);
        println!("{}", s);
    }
}
