/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_types::region::{self, ProvideRegion};
use chrono::prelude::DateTime;
use chrono::Utc;
use cognitoidentity::{Client, Config, Error, Region, PKG_VERSION};
//use std::time::{Duration, SystemTime, UNIX_EPOCH};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The AWS Region.
    #[structopt(short, long)]
    region: Option<String>,

    /// The ID of the identity pool to describe.
    #[structopt(short, long)]
    identity_pool_id: String,

    /// Whether to display additional information.
    #[structopt(short, long)]
    verbose: bool,
}

/// Lists the identities in an Amazon Cognito identitiy pool.
/// # Arguments
///
/// * `-i IDENTITY-POOL-ID` - The ID of the identity pool.
/// * `[-r REGION]` - The Region in which the client is created.
///   If not supplied, uses the value of the **AWS_REGION** environment variable.
///   If the environment variable is not set, defaults to **us-west-2**.
/// * `[-v]` - Whether to display additional information.
#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt::init();

    let Opt {
        identity_pool_id,
        region,
        verbose,
    } = Opt::from_args();

    let region_provider = region::ChainProvider::first_try(region.map(Region::new))
        .or_default_provider()
        .or_else(Region::new("us-west-2"));

    println!();

    if verbose {
        println!("Cognito client version: {}", PKG_VERSION);
        println!(
            "Region:                 {}",
            region_provider.region().unwrap().as_ref()
        );
        println!("Identity pool ID:       {}", identity_pool_id);
        println!();
    }

    let config = Config::builder().region(region_provider).build();
    let client = Client::from_conf(config);

    let response = client
        .list_identities()
        .identity_pool_id(identity_pool_id)
        .max_results(10)
        .send()
        .await?;

    let pool_id = response.identity_pool_id.unwrap_or_default();
    println!("Pool ID: {}", pool_id);
    println!();

    if let Some(ids) = response.identities {
        println!("Identitities:");
        for id in ids {
            let creation_date = id.creation_date.unwrap().to_system_time().unwrap();
            let creation_datetime = DateTime::<Utc>::from(creation_date);
            // Formats the combined date and time with the specified format string.
            let creation_timestamp_str =
                creation_datetime.format("%Y-%m-%d %H:%M:%S.%f").to_string();

            let idid = id.identity_id.unwrap_or_default();
            let mod_date = id.last_modified_date.unwrap().to_system_time().unwrap();
            let mod_datetime = DateTime::<Utc>::from(mod_date);
            let mod_timestamp_str = mod_datetime.format("%Y-%m-%d %H:%M:%S.%f").to_string();

            println!("  Creation data:      {}", creation_timestamp_str);
            println!("  ID:                 {}", idid);
            println!("  Last modified data: {}", mod_timestamp_str);

            if let Some(logins) = id.logins {
                println!("  Logins:");
                for login in logins {
                    println!("    {}", login);
                }
            }

            println!();
        }
    }

    let next_token = response.next_token;

    println!("Next token: {:?}", next_token);

    println!();

    Ok(())
}
