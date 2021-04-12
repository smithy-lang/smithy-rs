/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::process;

use aws_hyper::SdkError;

use kms::error::{GenerateDataKeyWithoutPlaintextError, GenerateDataKeyWithoutPlaintextErrorKind};
use kms::model::DataKeySpec;

use kms::{Client, Region};

use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

async fn display_error_hint(client: &Client, err: GenerateDataKeyWithoutPlaintextError) {
    eprintln!("Error while decrypting: {}", err);
    match err.kind {
        GenerateDataKeyWithoutPlaintextErrorKind::NotFoundError(_) => {
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

    /// Specifies whether to display additonal runtime information
    #[structopt(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();

    if opt.verbose {
        println!("GenerateDataKeyWithoutPlaintext called with options:");
        println!("  Region:  {}", opt.region);
        println!("  KMS key: {}", opt.key);

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

    let resp = match client
        .generate_data_key_without_plaintext()
        .key_id(opt.key)
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
