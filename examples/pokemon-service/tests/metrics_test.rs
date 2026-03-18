/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use pokemon_service_client::Client;
use serial_test::serial;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::time::Duration;

pub mod common;

/// E2E integration tests for aws_smithy_http_server_metrics using the pokemon service
#[tokio::test]
#[serial]
async fn test_metrics_content_via_tcp() {
    // Start TCP listener to receive metrics
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Spawn a task to collect metrics into the buffer
    let metrics_buffer = Arc::new(Mutex::new(Vec::new()));
    spawn_metrics_collection_task(listener, Arc::clone(&metrics_buffer)).await;

    // Start server with metrics TCP address
    let server = common::run_server_with_metrics_tcp(&addr.to_string()).await;
    let client = common::client(server.port);

    send_requests(client).await;

    // Poll for the metrics with a timeout of 5 seconds
    let expected_metrics_count = 3;
    let timeout = Duration::from_secs(5);
    let metrics_output = poll_for_metrics(metrics_buffer, expected_metrics_count, timeout).await;

    assert!(
        !metrics_output.is_empty(),
        "Expected metrics to be captured"
    );

    insta::with_settings!({filters => vec![
        (r#""Timestamp":\d+"#, r#""Timestamp":"[timestamp]"#),
        (r#""operation_time":[\d.]+"#, r#""operation_time":"[operation_time]"#),
        (r#""request_id":"[0-9a-f-]+""#, r#""request_id":"[request_id]"#),
    ]}, {
        insta::assert_snapshot!(metrics_output);
    });
}

async fn spawn_metrics_collection_task(listener: TcpListener, metrics_buffer: Arc<Mutex<Vec<u8>>>) {
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = vec![0u8; 8192];
            while let Ok(n) = socket.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                metrics_buffer.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        }
    });
}

async fn send_requests(client: Client) {
    // Send a successful request
    let _ = client.get_pokemon_species().name("pikachu").send().await;

    // Send a request with an invalid password to get a 400 level error
    let _ = client
        .get_storage()
        .user("ash")
        .passcode("pkachu123")
        .send()
        .await;

    // Send a request with an invalid region to get 500 level error
    let events =
        aws_smithy_http::event_stream::EventStreamSender::from(futures_util::stream::empty());
    let _ = client
        .capture_pokemon()
        .region("trigger500")
        .events(events)
        .send()
        .await;
}

async fn poll_for_metrics(
    metrics_buffer: Arc<Mutex<Vec<u8>>>,
    expected_metrics_count: usize,
    timeout: Duration,
) -> String {
    let start = tokio::time::Instant::now();

    let mut found_metrics_count = 0;
    loop {
        let buffer_data = metrics_buffer.lock().unwrap().clone();
        if !buffer_data.is_empty() {
            let output = String::from_utf8_lossy(&buffer_data);
            found_metrics_count = output.lines().filter(|l| !l.is_empty()).count();
            if found_metrics_count == expected_metrics_count {
                break;
            } else if found_metrics_count > expected_metrics_count {
                panic!("Found more metrics entries than expected, found {found_metrics_count}, expected {expected_metrics_count}");
            }
        }

        if start.elapsed() > timeout {
            panic!("Timeout waiting for expected number of metrics entries, found {found_metrics_count} after {} seconds, expected {expected_metrics_count}", timeout.as_secs());
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }

    // Parse metrics as JSON lines (EMF format)
    String::from_utf8(metrics_buffer.lock().unwrap().clone()).unwrap()
}
