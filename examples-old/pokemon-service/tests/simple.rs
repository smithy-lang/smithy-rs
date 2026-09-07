/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use bytes;
use http_body_util;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use pokemon_service_client::{
    error::{DisplayErrorContext, SdkError},
    operation::get_storage::GetStorageError,
    types::error::StorageAccessNotAuthorized,
};

pub mod common;

#[tokio::test]
async fn simple_integration_test() {
    let server = common::run_server().await;
    let client = common::client(server.port);

    let service_statistics_out = client.get_server_statistics().send().await.unwrap();
    assert_eq!(0, service_statistics_out.calls_count);

    let pokemon_species_output = client
        .get_pokemon_species()
        .name("pikachu")
        .send()
        .await
        .unwrap();
    assert_eq!("pikachu", pokemon_species_output.name());

    let service_statistics_out = client.get_server_statistics().send().await.unwrap();
    assert_eq!(1, service_statistics_out.calls_count);

    let storage_err = client
        .get_storage()
        .user("ash")
        .passcode("pikachu321")
        .send()
        .await;
    let has_not_authorized_error = if let Err(SdkError::ServiceError(context)) = storage_err {
        matches!(
            context.err(),
            GetStorageError::StorageAccessNotAuthorized(StorageAccessNotAuthorized { .. }),
        )
    } else {
        false
    };
    assert!(has_not_authorized_error, "expected NotAuthorized error");

    let storage_out = client
        .get_storage()
        .user("ash")
        .passcode("pikachu123")
        .send()
        .await
        .unwrap();
    assert_eq!(
        vec![
            "bulbasaur".to_string(),
            "charmander".to_string(),
            "squirtle".to_string(),
            "pikachu".to_string()
        ],
        storage_out.collection
    );

    let pokemon_species_error = client
        .get_pokemon_species()
        .name("some_pokémon")
        .send()
        .await
        .unwrap_err();
    let message = DisplayErrorContext(pokemon_species_error).to_string();
    let expected =
        r#"ResourceNotFoundError [ResourceNotFoundException]: Requested Pokémon not available"#;
    assert!(
        message.contains(expected),
        "expected '{message}' to contain '{expected}'"
    );

    let service_statistics_out = client.get_server_statistics().send().await.unwrap();
    assert_eq!(2, service_statistics_out.calls_count);

    let hyper_client = Client::builder(TokioExecutor::new()).build_http();
    let health_check_url = format!("{}/ping", common::base_url(server.port));
    let health_check_url = hyper::Uri::try_from(health_check_url).unwrap();
    let request = hyper::Request::builder()
        .uri(health_check_url)
        .body(http_body_util::Empty::<bytes::Bytes>::new())
        .unwrap();
    let result = hyper_client.request(request).await.unwrap();

    assert_eq!(result.status(), 200);
}

#[tokio::test]
async fn health_check() {
    let server = common::run_server().await;

    let url = common::base_url(server.port) + "/ping";
    let uri = url.parse::<hyper::Uri>().expect("invalid URL");

    // Since the `/ping` route is not modeled in Smithy, we use a regular
    // Hyper HTTP client to make a request to it.
    let request = hyper::Request::builder()
        .uri(uri)
        .body(http_body_util::Empty::<bytes::Bytes>::new())
        .expect("failed to build request");

    let client = Client::builder(TokioExecutor::new()).build_http();
    let response = client
        .request(request)
        .await
        .expect("failed to get response");

    assert_eq!(response.status(), hyper::StatusCode::OK);
    let body = http_body_util::BodyExt::collect(response.into_body())
        .await
        .expect("failed to read response body")
        .to_bytes();
    assert!(body.is_empty());
}
