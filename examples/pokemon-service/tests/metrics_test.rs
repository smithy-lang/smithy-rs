/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use serde_json::Value;
use serial_test::serial;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::time::Duration;

pub mod common;

/// E2E integration test for aws_smithy_http_server_metrics using the pokemon service
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
    let _child = common::run_server_with_metrics_tcp(&addr.to_string()).await;

    send_requests().await;

    // Poll for the metrics with a timeout of 5 seconds
    let timeout = Duration::from_secs(5);
    let metrics_output = poll_for_metrics(metrics_buffer, timeout).await;

    let metrics = parse_metrics(metrics_output);

    assert!(!metrics.is_empty(), "Expected metrics to be captured");

    test_get_pokemon_species_metrics(&metrics);
    test_get_storage_metrics(&metrics);
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

async fn send_requests() {
    let client = common::client();

    client
        .get_pokemon_species()
        .name("pikachu")
        .send()
        .await
        .unwrap();

    client
        .get_storage()
        .user("ash")
        .passcode("pikachu123")
        .send()
        .await
        .unwrap();
}

async fn poll_for_metrics(metrics_buffer: Arc<Mutex<Vec<u8>>>, timeout: Duration) -> String {
    let start = tokio::time::Instant::now();

    let expected_metrics_count = 2;
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
            panic!("Timeout waiting for expected number of metrics entries, found {found_metrics_count} after 5 seconds, expected {expected_metrics_count}", );
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }

    // Parse metrics as JSON lines (EMF format)
    String::from_utf8(metrics_buffer.lock().unwrap().clone()).unwrap()
}

fn parse_metrics(metrics_output: String) -> Vec<Value> {
    metrics_output
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).expect("Failed to parse EMF JSON"))
        .collect()
}

fn test_get_pokemon_species_metrics(metrics: &Vec<Value>) {
    let get_pokemon_species_metrics = metrics
        .iter()
        .find(|m| m["operation_name"] == "GetPokemonSpecies")
        .expect("Expected GetPokemonSpecies metrics");

    get_pokemon_species_metrics
        .get("_aws")
        .unwrap()
        .get("CloudWatchMetrics")
        .unwrap();

    assert_eq!(
        get_pokemon_species_metrics["service_name"],
        "PokemonService"
    );
    assert_eq!(
        get_pokemon_species_metrics["operation_name"],
        "GetPokemonSpecies"
    );
    assert_eq!(
        get_pokemon_species_metrics["requested_pokemon_name"],
        "pikachu"
    );
    assert_eq!(get_pokemon_species_metrics["found"], true);
}

fn test_get_storage_metrics(metrics: &Vec<Value>) {
    let storage_metric = metrics
        .iter()
        .find(|m| m["operation_name"] == "GetStorage")
        .expect("Expected GetStorage metrics");

    assert_eq!(storage_metric["operation_name"], "GetStorage");
    assert_eq!(storage_metric["user"], "ash");
    assert_eq!(storage_metric["authenticated"], true);
}
