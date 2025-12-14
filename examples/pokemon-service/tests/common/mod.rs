/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    time::Duration,
};

use tokio::time::timeout;

use pokemon_service::DEFAULT_ADDRESS;
use pokemon_service_client::{Client, Config};
use pokemon_service_common::ChildDrop;

pub struct ServerHandle {
    pub child: ChildDrop,
    pub port: u16,
}

pub async fn run_server() -> ServerHandle {
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("pokemon-service"))
        .args(["--port", "0"]) // Use port 0 for random available port
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // Wait for the server to signal it's ready by reading stderr
    let stderr = child.stderr.take().unwrap();
    let ready_signal = tokio::task::spawn_blocking(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if let Ok(line) = line {
                if let Some(port_str) = line.strip_prefix("SERVER_READY:") {
                    if let Ok(port) = port_str.parse::<u16>() {
                        return Some(port);
                    }
                }
            }
        }
        None
    });

    // Wait for the ready signal with a timeout
    let port = match timeout(Duration::from_secs(5), ready_signal).await {
        Ok(Ok(Some(port))) => port,
        _ => {
            panic!("Server did not become ready within 5 seconds");
        }
    };

    ServerHandle {
        child: ChildDrop(child),
        port,
    }
}

pub fn base_url(port: u16) -> String {
    format!("http://{DEFAULT_ADDRESS}:{port}")
}

pub fn client(port: u16) -> Client {
    let config = Config::builder()
        .endpoint_url(format!("http://{DEFAULT_ADDRESS}:{port}"))
        .build();
    Client::from_conf(config)
}
