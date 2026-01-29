/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
/// This example demonstrates how to use a mock connector with `capture_request`. This allows for
/// responding with a static `Response` while capturing the incoming request. The captured request
/// can later be asserted to verify that the correct headers and body were sent to the server.
///
/// The example assumes that the Pokémon service is running on the localhost on TCP port 13734.
/// Refer to the [README.md](https://github.com/smithy-lang/smithy-rs/tree/main/examples/pokemon-service-client-usage/README.md)
/// file for instructions on how to launch the service locally.
///
/// The example can be run using `cargo run --example mock-request`.
///
use aws_smithy_http_client::test_util::capture_request;
use pokemon_service_client::primitives::SdkBody;
use pokemon_service_client::Client as PokemonClient;
use pokemon_service_client_usage::{setup_tracing_subscriber, POKEMON_SERVICE_URL};

#[tokio::main]
async fn main() {
    setup_tracing_subscriber();

    // Build a response that should be sent when the operation is called.
    let response = http::Response::builder()
        .status(200)
        .body(SdkBody::from(r#"{"calls_count":100}"#))
        .expect("response could not be constructed");

    // Call `capture_request` to obtain a HTTP connector and a request receiver.
    // The request receiver captures the incoming request, while the connector can be passed
    // to `Config::builder().http_client`.
    let (http_client, captured_request) = capture_request(Some(response));

    // Pass the `http_client` connector to `Config::builder`. The connector won't send
    // the request over the network; instead, it will return the static response provided
    // during its initialization.
    let config = pokemon_service_client::Config::builder()
        .endpoint_url(POKEMON_SERVICE_URL)
        .http_client(http_client)
        .build();

    // Instantiate a client by applying the configuration.
    let client = PokemonClient::from_conf(config);

    // Call an operation `get_server_statistics` on the Pokémon service.
    let response = client
        .get_server_statistics()
        .customize()
        .mutate_request(|req| {
            // For demonstration, send an extra header that can be verified to confirm
            // that the client actually sends it.
            let headers = req.headers_mut();
            headers.insert(
                hyper::header::HeaderName::from_static("user-agent"),
                hyper::header::HeaderName::from_static("sample-client"),
            );
        })
        .send()
        .await
        .expect("operation failed");

    // Print the response received from the service.
    tracing::info!(%POKEMON_SERVICE_URL, ?response, "Response received");

    // The captured request can be verified to have certain headers.
    let req = captured_request.expect_request();
    assert_eq!(req.headers().get("user-agent"), Some("sample-client"));

    // As an example, you can verify the URL matches.
    assert_eq!(req.uri(), "http://localhost:13734/stats");

    // You can convert the captured body into a &str and use assert!
    // on it if you want to verify the contents of the request body.
    // let str_body = std::str::from_utf8(req.body().bytes().unwrap()).unwrap();
}
