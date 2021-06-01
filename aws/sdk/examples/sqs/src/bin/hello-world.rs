/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::process::exit;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::SubscriberBuilder;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), sqs::Error> {
    SubscriberBuilder::default()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let client = sqs::Client::from_env();
    let queues = client.list_queues().send().await?;
    let mut queue_urls = queues.queue_urls.unwrap_or_default();
    let queue_url = match queue_urls.pop() {
        Some(url) => url,
        None => {
            eprintln!("No queues in this account. Please create a queue to proceed");
            exit(1);
        }
    };
    println!("sending a receiving on `{}`", queue_url);

    let rsp = client
        .send_message()
        .queue_url(&queue_url)
        .message_body("hello from my queue")
        .send()
        .await?;
    println!("sent a message: {:#?}", rsp);

    let rcv_message_output = client
        .receive_message()
        .max_number_of_messages(5)
        .queue_url(&queue_url)
        .send()
        .await?;
    for message in rcv_message_output.messages.unwrap_or_default() {
        println!("got a message: {:#?}", message);
    }
    Ok(())
}
