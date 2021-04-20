/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */
use std::process;

use aws_hyper::SdkError;

use kms::error::{GenerateDataKeyWithoutPlaintextError, GenerateDataKeyWithoutPlaintextErrorKind};
use kms::model::DataKeySpec;

use kms::{Client, Config, Region};

use aws_types::region::{EnvironmentProvider, ProvideRegion};

use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

async fn display_error_hint(client: &Client, err: GenerateDataKeyWithoutPlaintextError) {
    eprintln!("Error while decrypting: {}", err);
    if let GenerateDataKeyWithoutPlaintextErrorKind::NotFoundError(_) = err.kind {
        client
            .list_keys()
            .send()
            .await
            .expect("failure to list keys");
    }
}

#[derive(Debug, StructOpt)]
struct Opt {
    /// The region
    #[structopt(short, long)]
    region: Option<String>,

    /// Specifies the encryption key
    #[structopt(short, long)]
    key: String,

    /// Specifies whether to display additonal runtime information
    #[structopt(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let Opt {
        key,
        region,
        verbose,
    } = Opt::from_args();

    let region = EnvironmentProvider::new()
        .region()
        .or_else(|| region.as_ref().map(|region| Region::new(region.clone())))
        .unwrap_or_else(|| Region::new("us-west-2"));

    if verbose {
        println!("GenerateDataKeyWithoutPlaintext called with options:");
        println!("Region:  {:?}", &region);
        println!("KMS key: {}", key);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let config = Config::builder().region(region).build();
    let client = kms::Client::from_conf(config);

    let resp = match client
        .generate_data_key_without_plaintext()
        .key_id(key)
        .key_spec(DataKeySpec::Aes256)
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

    println!("\nData key:");
    println!("{}", s);
}
