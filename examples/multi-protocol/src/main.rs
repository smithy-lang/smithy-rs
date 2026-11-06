/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! One generated client selects five protocols at runtime against one local server.

use aws_smithy_cbor::protocol::RpcV2CborProtocol;
use aws_smithy_json::protocol::{
    aws_json_rpc::AwsJsonRpcProtocol, aws_rest_json_1::AwsRestJsonProtocol,
};
use aws_smithy_schema::protocol::SharedClientProtocol;
use aws_smithy_xml::protocol::aws_rest_xml::AwsRestXmlProtocol;
use multi_protocol_client::{Client, Config};
use multi_protocol_server_sdk::{
    error::GreetError, input::GreetInput, output::GreetOutput, MultiProtocolService,
    MultiProtocolServiceConfig,
};

async fn greet(input: GreetInput) -> Result<GreetOutput, GreetError> {
    let name = input.name.into_inner();
    let greeting = input.greeting.unwrap_or_else(|| "hello".to_owned());
    let times = input.times.unwrap_or(1);
    Ok(GreetOutput {
        message: format!("{greeting} {name} x{times}"),
        greeted: Some(name),
        tags: input.tags,
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}", listener.local_addr()?);
    // The showcase calls Greet; the full service fixture lives in protocol-compat.
    let service = MultiProtocolService::builder(MultiProtocolServiceConfig::builder().build())
        .greet(greet)
        .build_unchecked();
    let server = tokio::spawn(async move {
        multi_protocol_server_sdk::serve(listener, service.into_make_service()).await
    });

    let protocols = [
        (
            "REST JSON",
            SharedClientProtocol::new(AwsRestJsonProtocol::new()),
        ),
        (
            "REST XML",
            SharedClientProtocol::new(AwsRestXmlProtocol::new()),
        ),
        (
            "AWS JSON 1.0",
            SharedClientProtocol::new(AwsJsonRpcProtocol::aws_json_1_0()),
        ),
        (
            "AWS JSON 1.1",
            SharedClientProtocol::new(AwsJsonRpcProtocol::aws_json_1_1()),
        ),
        (
            "RPCv2 CBOR",
            SharedClientProtocol::new(RpcV2CborProtocol::new()),
        ),
    ];
    for (name, protocol) in protocols {
        // Every Client has the same generated type. Its configuration selects the wire protocol.
        let mut config = Config::builder().endpoint_url(&endpoint);
        config.set_protocol(Some(protocol));
        let client = Client::from_conf(config.build());
        let output = client
            .greet()
            .name("ada")
            .greeting("hi")
            .times(2)
            .send()
            .await?;
        println!("{name}: {}", output.message());
    }
    server.abort();
    Ok(())
}
