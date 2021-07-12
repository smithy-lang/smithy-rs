/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use cloudwatchlogs::{model::InputLogEvent, Client, Config, Region};
use std::time::{SystemTime, UNIX_EPOCH};

#[tokio::main]
async fn main() -> Result<(), cloudwatchlogs::Error> {
    tracing_subscriber::fmt::init();

    let conf = Config::builder().region(Region::new("us-east-1")).build();
    let client = Client::from_conf(conf);
    let start = SystemTime::now();
    let timestamp: i64 = start
        .duration_since(UNIX_EPOCH)
        .expect("Time Issue")
        .as_millis() as i64;
    let log_group = "test-group".to_string();
    let log_stream = "test-stream".to_string();

    let message = format!(
        "{{
        \"_aws\": {{
        \"Timestamp\": {},
        \"CloudWatchMetrics\": [
        {{
            \"Namespace\": \"lambda-function-metrics\",
            \"Dimensions\": [[\"functionVersion\"]],
            \"Metrics\": [
            {{
                \"Name\": \"time\",
                \"Unit\": \"Milliseconds\"
            }}
            ]
        }}
        ]
    }},
    \"functionVersion\": \"$LATEST\",
    \"time\": 100,
    \"requestId\": \"989ffbf8-9ace-4817-a57c-e4dd734019ee\"
    }}",
        timestamp
    );

    let input_event = InputLogEvent::builder()
        .timestamp(timestamp)
        .message(message)
        .build();
    let seq_token = get_next_seq_token(&client, &log_group, &log_stream).await?;
    let log_events = client
        .put_log_events()
        .log_events(input_event)
        .log_group_name(log_group)
        .log_stream_name(log_stream)
        .sequence_token(seq_token)
        .send()
        .await?;

    println!("{:?}", log_events);
    println!(
        "next_sequence_token: {}",
        log_events.next_sequence_token.unwrap_or_default()
    );

    Ok(())
}

async fn get_next_seq_token(
    client: &Client,
    log_group: &String,
    log_stream: &String,
) -> Result<String, cloudwatchlogs::Error> {
    let seq_token = client
        .describe_log_streams()
        .log_group_name(log_group)
        .log_stream_name_prefix(log_stream)
        .send()
        .await?
        .log_streams
        .unwrap()
        // Assume that a single stream is returned since an exact stream name was specified in the previous request
        .get(0)
        .unwrap()
        .upload_sequence_token
        .as_deref()
        .unwrap()
        .to_string();

    Ok(seq_token)
}
