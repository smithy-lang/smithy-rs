/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_auth_providers::DefaultProviderChain;
use aws_sdk_s3::model::{
    CompressionType, CsvInput, CsvOutput, ExpressionType, FileHeaderInfo, InputSerialization,
    OutputSerialization, SelectObjectContentEventStream,
};
use aws_sdk_s3::{Client, Config, Error, Region, PKG_VERSION};
use aws_types::region::{default_provider, ProvideRegion};

use structopt::StructOpt;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The AWS Region.
    #[structopt(short, long)]
    region: Option<String>,

    /// The bucket name to select from.
    #[structopt(short, long)]
    bucket: String,

    /// The object key to scan. This example expects the object to be an uncompressed CSV file with:
    /// ```csv
    /// Name,PhoneNumber,City,Occupation
    /// Sam,(949) 555-6701,Irvine,Solutions Architect
    /// Vinod,(949) 555-6702,Los Angeles,Solutions Architect
    /// Jeff,(949) 555-6703,Seattle,AWS Evangelist
    /// Jane,(949) 555-6704,Chicago,Developer
    /// Sean,(949) 555-6705,Chicago,Developer
    /// Mary,(949) 555-6706,Chicago,Developer
    /// Kate,(949) 555-6707,Chicago,Developer
    /// ```
    #[structopt(short, long)]
    object: String,

    /// Whether to display additional information.
    #[structopt(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt::init();

    let Opt {
        region,
        bucket,
        object,
        verbose,
    } = Opt::from_args();

    let region = region::ChainProvider::first_try(region.map(Region::new))
        .or_default_provider()
        .or_else(Region::new("us-east-2"));

    println!();

    if verbose {
        println!("S3 client version: {}", PKG_VERSION);
        println!(
            "Region:            {}",
            region.region().await.unwrap().as_ref()
        );
        println!();
    }

    let credential_provider = DefaultProviderChain::builder()
        .region(region.clone())
        .build();

    let config = Config::builder()
        .region(region)
        .credentials_provider(credential_provider)
        .build();

    let client = Client::from_conf(config);

    let mut output = client
        .select_object_content()
        .bucket(bucket)
        .key(object)
        .expression_type(ExpressionType::Sql)
        .expression("SELECT * FROM s3object s WHERE s.\"Name\" = 'Jane'")
        .input_serialization(
            InputSerialization::builder()
                .csv(
                    CsvInput::builder()
                        .file_header_info(FileHeaderInfo::Use)
                        .build(),
                )
                .compression_type(CompressionType::None)
                .build(),
        )
        .output_serialization(
            OutputSerialization::builder()
                .csv(CsvOutput::builder().build())
                .build(),
        )
        .send()
        .await?;

    while Some(event) = output.payload.recv().await? {
        match event {
            SelectObjectContentEventStream::Records(records) => {
                println!(
                    "Record: {}",
                    records
                        .payload
                        .as_ref()
                        .map(|p| std::str::from_utf8(p.as_ref()).unwrap())
                        .unwrap_or("")
                );
            }
            SelectObjectContentEventStream::Stats(stats) => {
                println!("Stats: {:?}", stats.details.unwrap());
            }
            SelectObjectContentEventStream::Progress(progress) => {
                println!("Progress: {:?}", progress.details.unwrap());
            }
            SelectObjectContentEventStream::Cont(_) => {
                println!("Continuation Event");
            }
            SelectObjectContentEventStream::End(_) => {
                println!("End Event");
            }
            otherwise => panic!("Unknown event type: {:?}", otherwise),
        }
    }

    Ok(())
}
