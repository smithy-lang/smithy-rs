/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use std::net::SocketAddr;

use aws_smithy_http_server::{body::Body, routing::Route};
use rpcv2_server_sdk::{error, input, output, RpcV2Service};

async fn handler(
    input: input::RpcV2OperationInput,
) -> Result<output::RpcV2OperationOutput, error::RpcV2OperationError> {
    println!("{input:#?}");

    todo!()
}

#[tokio::main]
async fn main() {
    let service: RpcV2Service<Route<Body>> = rpcv2_server_sdk::RpcV2Service::builder_without_plugins()
        .rpc_v2_operation(handler)
        .build()
        .unwrap();

    let server = service.into_make_service();
    let bind: SocketAddr = "127.0.0.1:6969"
        .parse()
        .expect("unable to parse the server bind address and port");

    println!("Binding {bind}");
    hyper::Server::bind(&bind).serve(server).await.unwrap();
}
