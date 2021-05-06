/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */
use std::fs;
use std::process;

use polly::model::{OutputFormat, VoiceId};
use polly::{Client, Config, Region};

use aws_types::region::ProvideRegion;

use structopt::StructOpt;
use tokio::io::AsyncWriteExt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The region. Overrides environment variable AWS_DEFAULT_REGION.
    #[structopt(short, long)]
    default_region: Option<String>,

    /// The input file containing the text to synthesize
    #[structopt(short, long)]
    filename: String,

    /// Whether to show additional output
    #[structopt(short, long)]
    verbose: bool,
}

/// Synthesizes the text from a file to a stream of bytes.
/// # Arguments
///
/// * `[-f FILENAME]` - The file containing the text to synthesize.
/// * `[-d DEFAULT-REGION]` - The region in which the client is created.
///    If not supplied, uses the value of the **AWS_DEFAULT_REGION** environment variable.
///    If the environment variable is not set, defaults to **us-west-2**.
/// * `[-v]` - Whether to display additional information.
/// # Output
/// The bytes are stored in a file with the same basename as FILENAME, with a .mp3 suffix.
#[tokio::main]
async fn main() {
    let Opt {
        filename,
        default_region,
        verbose,
    } = Opt::from_args();

    let region = default_region
        .as_ref()
        .map(|region| Region::new(region.clone()))
        .or_else(|| aws_types::region::default_provider().region())
        .unwrap_or_else(|| Region::new("us-west-2"));

    if verbose {
        println!("polly client version: {}\n", polly::PKG_VERSION);
        println!("Region:   {:?}", &region);
        println!("Filename: {}", filename);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let conf = Config::builder().region(region).build();
    let conn = aws_hyper::conn::Standard::https();
    let client = Client::from_conf_conn(conf, conn);

    let content = fs::read_to_string(&filename);

    let resp = match client
        .synthesize_speech()
        .output_format(OutputFormat::Mp3)
        .text(content.unwrap())
        .voice_id(VoiceId::Joanna)
        .send()
        .await
    {
        Ok(output) => output,
        Err(e) => {
            println!("Got an error synthesizing speech:");
            println!("{}", e);
            process::exit(1);
        }
    };

    // Get MP3 data from response and save it
    let blob = resp.audio_stream.expect("failed to read data");

    let parts: Vec<&str> = filename.split('.').collect();
    let out_file = format!("{}{}", String::from(parts[0]), ".mp3");

    let mut file = tokio::fs::File::create(out_file.clone())
        .await
        .expect("failed to create file");

    let bytes = blob.as_ref();

    match file.write(bytes).await {
        Ok(_) => println!("Saved output to {}", out_file),
        Err(e) => {
            println!("Got an error saving output to {}:", out_file);
            println!("{}", e);
            process::exit(1);
        }
    }
}
