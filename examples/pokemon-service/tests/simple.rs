/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use pokemon_service_client::{
    error::{DisplayErrorContext, SdkError},
    operation::get_storage::GetStorageError,
    types::error::StorageAccessNotAuthorized,
};
use serial_test::serial;

pub mod common;

#[tokio::test]
#[serial]
async fn simple_integration_test() {
    let _child = common::run_server().await;
    let client = common::client();

    let service_statistics_out = client.get_server_statistics().send().await.unwrap();
    assert_eq!(0, service_statistics_out.calls_count.unwrap());

    let pokemon_species_output = client
        .get_pokemon_species()
        .name("pikachu")
        .send()
        .await
        .unwrap();
    assert_eq!("pikachu", pokemon_species_output.name().unwrap());

    let service_statistics_out = client.get_server_statistics().send().await.unwrap();
    assert_eq!(1, service_statistics_out.calls_count.unwrap());

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
        Some(vec![
            "bulbasaur".to_string(),
            "charmander".to_string(),
            "squirtle".to_string(),
            "pikachu".to_string()
        ]),
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
    assert_eq!(2, service_statistics_out.calls_count.unwrap());

    let hyper_client = hyper::Client::new();
    let health_check_url = format!("{}/ping", common::base_url());
    let health_check_url = hyper::Uri::try_from(health_check_url).unwrap();
    let result = hyper_client.get(health_check_url).await.unwrap();

    assert_eq!(result.status(), 200);
}

#[tokio::test]
#[serial]
async fn health_check() {
    let _child = common::run_server().await;

    use pokemon_service::{DEFAULT_ADDRESS, DEFAULT_PORT};
    let url = format!("http://{DEFAULT_ADDRESS}:{DEFAULT_PORT}/ping");
    let uri = url.parse::<hyper::Uri>().expect("invalid URL");

    // Since the `/ping` route is not modeled in Smithy, we use a regular
    // Hyper HTTP client to make a request to it.
    let request = hyper::Request::builder()
        .uri(uri)
        .body(hyper::Body::empty())
        .expect("failed to build request");

    let response = hyper::Client::new()
        .request(request)
        .await
        .expect("failed to get response");

    assert_eq!(response.status(), hyper::StatusCode::OK);
    let body = hyper::body::to_bytes(response.into_body())
        .await
        .expect("failed to read response body");
    assert!(body.is_empty());
}
