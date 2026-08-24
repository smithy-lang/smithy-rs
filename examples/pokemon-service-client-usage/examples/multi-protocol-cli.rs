/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use clap::{Parser, ValueEnum};
use pokemon_service_client::{Client, Config};
use pokemon_service_client_usage::{setup_tracing_subscriber, POKEMON_SERVICE_URL};

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Protocol {
    RpcV2Cbor,
    RestJson1,
    RestXml,
    All,
}

impl Protocol {
    const ALL: [Self; 3] = [Self::RpcV2Cbor, Self::RestJson1, Self::RestXml];

    fn name(self) -> &'static str {
        match self {
            Self::RpcV2Cbor => "rpcv2Cbor",
            Self::RestJson1 => "restJson1",
            Self::RestXml => "restXml",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Parser)]
#[command(about = "Invoke the multi-protocol Pokemon server")]
struct Args {
    /// Protocol to use for the request.
    #[arg(long, value_enum, default_value_t = Protocol::All)]
    protocol: Protocol,

    /// Pokemon service endpoint.
    #[arg(long, default_value = POKEMON_SERVICE_URL)]
    endpoint: String,
}

fn client(protocol: Protocol, endpoint: &str) -> Client {
    let builder = Config::builder().endpoint_url(endpoint);
    let config = match protocol {
        Protocol::RpcV2Cbor => builder
            .protocol(aws_smithy_cbor::protocol::RpcV2CborProtocol::new())
            .build(),
        Protocol::RestJson1 => builder
            .protocol(
                aws_smithy_json::protocol::aws_rest_json_1::AwsRestJsonProtocol::new()
                    .with_default_namespace("com.aws.example"),
            )
            .build(),
        Protocol::RestXml => builder
            .protocol(aws_smithy_xml::protocol::aws_rest_xml::AwsRestXmlProtocol::new())
            .build(),
        Protocol::All => unreachable!("all protocols are expanded before creating a client"),
    };
    Client::from_conf(config)
}

async fn invoke(protocol: Protocol, endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let response = client(protocol, endpoint)
        .get_server_statistics()
        .send()
        .await?;
    println!(
        "{}: calls_count={}",
        protocol.name(),
        response.calls_count()
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_tracing_subscriber();
    let args = Args::parse();

    if matches!(args.protocol, Protocol::All) {
        for protocol in Protocol::ALL {
            invoke(protocol, &args.endpoint).await?;
        }
    } else {
        invoke(args.protocol, &args.endpoint).await?;
    }

    Ok(())
}
