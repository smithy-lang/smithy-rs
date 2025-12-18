/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{process::Command, time::Duration};

use assert_cmd::prelude::*;
use tokio::time::sleep;

use pokemon_service::{DEFAULT_ADDRESS, DEFAULT_PORT};
use pokemon_service_client::{Client, Config};
use pokemon_service_common::ChildDrop;

pub async fn run_server() -> ChildDrop {
    let crate_name = std::env::var("CARGO_PKG_NAME").unwrap();
    let child = Command::cargo_bin(crate_name).unwrap().spawn().unwrap();

    sleep(Duration::from_millis(500)).await;

    ChildDrop(child)
}

pub fn base_url() -> String {
    format!("http://{DEFAULT_ADDRESS}:{DEFAULT_PORT}")
}

pub fn client() -> Client {
    let config = Config::builder()
        .endpoint_url(format!("http://{DEFAULT_ADDRESS}:{DEFAULT_PORT}"))
        .build();
    Client::from_conf(config)
}
