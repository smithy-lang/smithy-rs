/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::process;

use polly::{Client, Config, Region};

use aws_types::region::ProvideRegion;

use structopt::StructOpt;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;

#[derive(Debug, StructOpt)]
struct Opt {
    /// The region. Overrides environment variable AWS_DEFAULT_REGION.
    #[structopt(short, long)]
    default_region: Option<String>,

    /// The name of the lexicon
    #[structopt(short, long)]
    name: String,

    /// The word to replace
    #[structopt(short, long)]
    from_text: String,

    /// The replacement
    #[structopt(short, long)]
    to_text: String,

    /// Whether to show additional output
    #[structopt(short, long)]
    verbose: bool,
}

/// Creates a pronunciation lexicon.
/// # Arguments
///
/// * `[-n NAME]` - The name of the new lexicon.
/// * `[-f FROM-TEXT]` - The expression to replace.
/// * `[-t TO-TEXT]` - The expression to replace with.
/// * `[-d DEFAULT-REGION]` - The region in which the client is created.
///    If not supplied, uses the value of the **AWS_DEFAULT_REGION** environment variable.
///    If the environment variable is not set, defaults to **us-west-2**.
/// * `[-v]` - Whether to display additional information.
#[tokio::main]
async fn main() {
    let Opt {
        from_text,
        name,
        default_region,
        to_text,
        verbose,
    } = Opt::from_args();

    let region = default_region
        .as_ref()
        .map(|region| Region::new(region.clone()))
        .or_else(|| aws_types::region::default_provider().region())
        .unwrap_or_else(|| Region::new("us-west-2"));

    if verbose {
        println!("polly client version: {}\n", polly::PKG_VERSION);
        println!("Region:           {:?}", &region);
        println!("Lexicon name:     {}", name);
        println!("Text to replace:  {}", from_text);
        println!("Replacement text: {}", to_text);

        SubscriberBuilder::default()
            .with_env_filter("info")
            .with_span_events(FmtSpan::CLOSE)
            .init();
    }

    let conf = Config::builder().region(region).build();
    let conn = aws_hyper::conn::Standard::https();
    let client = Client::from_conf_conn(conf, conn);

    let content = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>
    <lexicon version=\"1.0\" xmlns=\"http://www.w3.org/2005/01/pronunciation-lexicon\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"
    xsi:schemaLocation=\"http://www.w3.org/2005/01/pronunciation-lexicon http://www.w3.org/TR/2007/CR-pronunciation-lexicon-20071212/pls.xsd\"
    alphabet=\"ipa\" xml:lang=\"en-US\">
    <lexeme><grapheme>{}</grapheme><alias>{}</alias></lexeme>
    </lexicon>", from_text, to_text);

    match client
        .put_lexicon()
        .name(name)
        .content(content)
        .send()
        .await
    {
        Ok(_) => println!("Added lexicon"),
        Err(e) => {
            println!("Got an error adding lexicon:");
            println!("{}", e);
            process::exit(1);
        }
    };
}
