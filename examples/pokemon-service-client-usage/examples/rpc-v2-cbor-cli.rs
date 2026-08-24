use aws_smithy_cbor::protocol::RpcV2CborProtocol;

#[tokio::main]
async fn main() {
    let config = pokemon_service_client::Config::builder()
        .endpoint_url("http://localhost:13734")
        .protocol(RpcV2CborProtocol::new())
        .build();
    let client = pokemon_service_client::Client::from_conf(config);

    let result = client.get_server_statistics().send().await;
    println!("result={result:#?}");
}
