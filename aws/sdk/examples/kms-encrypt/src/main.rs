/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use clap::{App, Arg};

use std::process;

use aws_hyper::SdkError;
use kms::error::{EncryptError, EncryptErrorKind};
use kms::fluent::Client;
use kms::operation::Encrypt;
use kms::Blob;
use kms::Region;
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

#[tokio::main]
async fn main() {
    let matches = App::new("myapp")
        .arg(
            Arg::with_name("region")
                .short("r")
                .long("region")
                .value_name("REGION")
                .help("Specifies the region")
                .default_value("us-west-2")
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
                .help("Specifies the text to encrypt")
                .takes_value(true)
                .required(true),
        )
        .get_matches();

    let region = matches.value_of("region").expect("clap provides default");
    let key = matches.value_of("key").expect("marked required in clap");
    let text = matches.value_of("text").expect("marked required in clap");

    // TODO: (doug): Only enable logging if a `-v` flag is set
    /* SubscriberBuilder::default()
       .with_env_filter("info")
       .with_span_events(FmtSpan::CLOSE)
       .init();
    */
    let config = kms::Config::builder().region(Region::from(region)).build();

    let client = kms::fluent::Client::from_conf_conn(config, aws_hyper::conn::Standard::https());

    let blob = Blob::new(text.as_bytes());

    let resp = match client.encrypt().key_id(key).plaintext(blob).send().await {
        Ok(output) => output,
        Err(SdkError::ServiceError { err, .. }) => {
            display_error_hint(&client, err).await;
            process::exit(1);
        }
        Err(other) => panic!(""),
    };

    // Did we get an encrypted blob?
    // TODO doug: base64 encode this?
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
