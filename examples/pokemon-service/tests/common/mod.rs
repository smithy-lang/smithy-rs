/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{process::Command, time::Duration};

use assert_cmd::prelude::*;
use aws_smithy_client::erase::{DynConnector, DynMiddleware};
use tokio::time::sleep;

use pokemon_service::{DEFAULT_ADDRESS, DEFAULT_PORT};
use pokemon_service_client::{Builder, Client, Config};
use pokemon_service_common::{rewrite_base_url, ChildDrop};

pub async fn run_server() -> ChildDrop {
    let child = Command::cargo_bin("pokemon-service")
        .unwrap()
        .spawn()
        .unwrap();

    sleep(Duration::from_millis(500)).await;

    ChildDrop(child)
}

pub fn client() -> Client<DynConnector, DynMiddleware<DynConnector>> {
    let base_url = format!("http://{DEFAULT_ADDRESS}:{DEFAULT_PORT}");
    let raw_client = Builder::new()
        .rustls_connector(Default::default())
        .middleware_fn(rewrite_base_url(base_url))
        .build_dyn();
    let config = Config::builder().build();
    Client::with_config(raw_client, config)
}
