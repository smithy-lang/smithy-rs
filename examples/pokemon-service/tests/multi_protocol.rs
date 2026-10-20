/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http_server::body::Body;
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use pokemon_service_server_sdk::{
    error::GetPokemonSpeciesError, input::GetPokemonSpeciesInput, output::GetPokemonSpeciesOutput,
    PokemonService, PokemonServiceConfig,
};
use tower::ServiceExt;

#[tokio::test]
async fn pokemon_service_serves_every_built_in_protocol() {
    let app = PokemonService::builder(PokemonServiceConfig::builder().build())
        .get_pokemon_species(|input: GetPokemonSpeciesInput| async move {
            assert_eq!(input.name, "pikachu");
            Ok::<_, GetPokemonSpeciesError>(
                GetPokemonSpeciesOutput::builder()
                    .name(input.name)
                    .flavor_text_entries(vec![])
                    .build()
                    .unwrap(),
            )
        })
        .build_unchecked();

    let cases: &[(&str, &str, &str, &str, &[u8])] = &[
        (
            "restJson1",
            "GET",
            "/pokemon-species/pikachu",
            "application/json",
            b"",
        ),
        (
            "restXml",
            "GET",
            "/pokemon-species/pikachu",
            "application/xml",
            b"",
        ),
        (
            "awsJson1_0",
            "POST",
            "/",
            "application/x-amz-json-1.0",
            br#"{"name":"pikachu"}"#,
        ),
        (
            "awsJson1_1",
            "POST",
            "/",
            "application/x-amz-json-1.1",
            br#"{"name":"pikachu"}"#,
        ),
        (
            "rpcv2Cbor",
            "POST",
            "/service/PokemonService/operation/GetPokemonSpecies",
            "application/cbor",
            b"\xa1\x64name\x67pikachu",
        ),
    ];

    for &(protocol, method, uri, content_type, payload) in cases {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", content_type);
        if protocol.starts_with("awsJson") {
            request = request.header("x-amz-target", "PokemonService.GetPokemonSpecies");
        } else if protocol == "rpcv2Cbor" {
            request = request.header("smithy-protocol", "rpc-v2-cbor");
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(payload.to_vec())).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK, "{protocol}");
        assert_eq!(
            response.headers()["content-type"],
            content_type,
            "{protocol}"
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert!(
            body.windows(b"pikachu".len())
                .any(|window| window == b"pikachu"),
            "{protocol}: {body:?}"
        );
    }
}

#[tokio::test]
async fn pokemon_service_routes_bodyless_rest_operations() {
    let app = PokemonService::builder(PokemonServiceConfig::builder().build())
        .check_health(
            |_: pokemon_service_server_sdk::input::CheckHealthInput| async {
                pokemon_service_server_sdk::output::CheckHealthOutput {}
            },
        )
        .build_unchecked();

    // `CheckHealth` has no modeled input. Like generated serde servers (mainline answers
    // `415 UnsupportedMediaTypeException`), a bodiless request for it must not carry a
    // `Content-Type`; without one it is served.
    for (content_type, expected) in [
        (None, StatusCode::OK),
        (Some("application/json"), StatusCode::UNSUPPORTED_MEDIA_TYPE),
        (Some("application/xml"), StatusCode::UNSUPPORTED_MEDIA_TYPE),
    ] {
        let mut request = Request::builder().method("GET").uri("/ping");
        if let Some(content_type) = content_type {
            request = request.header("content-type", content_type);
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{content_type:?}");
    }
}
