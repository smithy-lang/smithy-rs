/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_types::region::ChainProvider;
use kms::model::DataKeySpec;
use kms::{Client, Config, Error, Region, PKG_VERSION};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The default AWS Region.
    #[structopt(short, long)]
    default_region: Option<String>,

    /// The encryption key.
    #[structopt(short, long)]
    key: String,

    /// Whether to display additonal informmation.
    #[structopt(short, long)]
    verbose: bool,
}

/// Creates an AWS KMS data key.
/// # Arguments
///
/// * `[-k KEY]` - The name of the key.
/// * `[-d DEFAULT-REGION]` - The Region in which the client is created.
///    If not supplied, uses the value of the **AWS_REGION** environment variable.
///    If the environment variable is not set, defaults to **us-west-2**.
/// * `[-v]` - Whether to display additional information.
#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt::init();

    let Opt {
        key,
        default_region,
        verbose,
    } = Opt::from_args();

    let region = ChainProvider::first_try(default_region.map(Region::new))
        .or_default_provider()
        .or_else(Region::new("us-west-2"));

    println!();

    if verbose {
        println!("KMS version: {}", PKG_VERSION);
        println!("Region:      {:?}", region.region().await);
        println!("Key:         {}", &key);
        println!();
    }

    let conf = Config::builder().region(region.region().await).build();
    let client = Client::from_conf(conf);

    let resp = client
        .generate_data_key()
        .key_id(key)
        .key_spec(DataKeySpec::Aes256)
        .send()
        .await?;

    // Did we get an encrypted blob?
    let blob = resp.ciphertext_blob.expect("Could not get encrypted text");
    let bytes = blob.as_ref();

    let s = base64::encode(&bytes);

    println!();
    println!("Data key:");
    println!("{}", s);

    Ok(())
}
