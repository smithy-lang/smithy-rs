/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use secretsmanager::{Client, Config, Region};

use aws_types::region::ProvideRegion;

use structopt::StructOpt;

use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The region. Overrides environment variable AWS_DEFAULT_REGION.
    #[structopt(short, long)]
    default_region: Option<String>,

    /// The name of the secret
    #[structopt(short, long)]
    name: String,

    /// The value of the secret
    #[structopt(short, long)]
    value: String,

    /// Whether to display additonal runtime information
    #[structopt(short, long)]
    info: bool,
}

/// Creates a secret.
/// # Arguments
///
/// * `[-n NAME]` - The name of the secret.
/// * `[-v VALUE]` - The value of the secret.
/// * `[-d DEFAULT-REGION]` - The region in which the client is created.
///    If not supplied, uses the value of the **AWS_DEFAULT_REGION** environment variable.
///    If the environment variable is not set, defaults to **us-west-2**.
/// * `[-i]` - Whether to display additional information.
#[tokio::main]
async fn main() {
    let Opt {
        info,
        name,
        default_region,
        value,
    } = Opt::from_args();

    let region = default_region
        .as_ref()
        .map(|region| Region::new(region.clone()))
        .or_else(|| aws_types::region::default_provider().region())
        .unwrap_or_else(|| Region::new("us-west-2"));

    if info {
        println!(
            "SecretsManager client version: {}\n",
            secretsmanager::PKG_VERSION
        );
        println!("Region:       {:?}", &region);
        println!("Secret name:  {}", name);
        println!("Secret value: {}", value);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let conf = Config::builder().region(region).build();
    let conn = aws_hyper::conn::Standard::https();
    let client = Client::from_conf_conn(conf, conn);

    match client
        .create_secret()
        .name(name)
        .secret_string(value)
        .send()
        .await
    {
        Ok(_) => println!("Created secret"),
        Err(e) => panic!("Failed to create secret: {}", e),
    };
}
