/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http_server::{
    extension::{ModeledErrorExtension, RuntimeErrorExtension},
    protocol::{
        aws_json_10::AwsJson1_0, aws_json_11::AwsJson1_1, rest_json_1::RestJson1, rest_xml::RestXml,
        rpc_v2_cbor::RpcV2Cbor,
    },
    response::{IntoResponse, Response},
    runtime_error::InternalFailureException,
    schema::{protocol, HttpModeledError, ServerProtocol},
};
use aws_smithy_schema::{
    serde::{SerdeError, SerializableStruct, ShapeSerializer},
    shape_id, Schema, ShapeType,
};
use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use http_body_util::BodyExt;

#[derive(Debug, PartialEq, Eq)]
struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
    modeled_error: Option<String>,
    runtime_error: Option<String>,
}

async fn snapshot(response: Response) -> Reply {
    let (parts, body) = response.into_parts();
    Reply {
        status: parts.status,
        headers: parts.headers,
        body: body.collect().await.unwrap().to_bytes(),
        modeled_error: parts
            .extensions
            .get::<ModeledErrorExtension>()
            .map(|error| (**error).to_owned()),
        runtime_error: parts
            .extensions
            .get::<RuntimeErrorExtension>()
            .map(|error| error.to_string()),
    }
}

static ERROR: Schema = Schema::new_struct(shape_id!("test", "Denied"), ShapeType::Structure, &[]);

#[derive(Debug, thiserror::Error)]
#[error("denied")]
struct Denied(u16);

impl SerializableStruct for Denied {
    fn schema(&self) -> &Schema<'_> {
        &ERROR
    }

    fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        Ok(())
    }
}

impl HttpModeledError for Denied {
    fn status_code(&self) -> u16 {
        self.0
    }
}

#[tokio::test]
async fn modeled_error_without_protocol_has_status_and_extension_but_no_framing() {
    let response = <Denied as IntoResponse<(RestJson1, RpcV2Cbor)>>::into_response(Denied(403));
    let reply = snapshot(response).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert!(reply.headers.is_empty());
    assert!(reply.body.is_empty());
    assert_eq!(reply.modeled_error.as_deref(), Some("Denied"));
    assert!(reply.runtime_error.is_none());
}

#[tokio::test]
async fn invalid_context_free_modeled_status_returns_internal_server_error() {
    let response = <Denied as IntoResponse<RestJson1>>::into_response(Denied(0));
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(response.headers().is_empty());
    assert!(response.into_body().collect().await.unwrap().to_bytes().is_empty());
}

#[test]
fn infallible_retains_response_support_through_the_blanket() {
    fn assert_response<P, E: IntoResponse<P>>() {}
    assert_response::<RestJson1, std::convert::Infallible>();
    assert_response::<(RestJson1, RpcV2Cbor), std::convert::Infallible>();
}

async fn check_tuple<P>()
where
    InternalFailureException: IntoResponse<P>,
{
    let cases: Vec<(Box<dyn ServerProtocol>, Response)> = vec![
        (
            Box::<protocol::RestJson1>::default(),
            IntoResponse::<RestJson1>::into_response(InternalFailureException),
        ),
        (
            Box::<protocol::AwsJson1_0>::default(),
            IntoResponse::<AwsJson1_0>::into_response(InternalFailureException),
        ),
        (
            Box::<protocol::AwsJson1_1>::default(),
            IntoResponse::<AwsJson1_1>::into_response(InternalFailureException),
        ),
        (
            Box::<protocol::RestXml>::default(),
            IntoResponse::<RestXml>::into_response(InternalFailureException),
        ),
        (
            Box::<protocol::RpcV2Cbor>::default(),
            IntoResponse::<RpcV2Cbor>::into_response(InternalFailureException),
        ),
    ];
    for (protocol, legacy) in cases {
        let selected = <InternalFailureException as IntoResponse<P>>::into_response_with_protocol(
            InternalFailureException,
            &*protocol,
        );
        assert_eq!(
            snapshot(selected).await,
            snapshot(legacy).await,
            "{}",
            std::any::type_name::<P>()
        );
    }
    let fallback = snapshot(<InternalFailureException as IntoResponse<P>>::into_response(
        InternalFailureException,
    ))
    .await;
    assert_eq!(fallback.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(fallback.headers.is_empty());
    assert!(fallback.body.is_empty());
    assert_eq!(fallback.runtime_error.as_deref(), Some("InternalFailureException"));
    assert!(fallback.modeled_error.is_none());
}

#[tokio::test]
async fn every_tuple_arity_through_ten_uses_selection_and_preserves_legacy_framing() {
    check_tuple::<(RestJson1, RpcV2Cbor)>().await;
    check_tuple::<(RpcV2Cbor, RestJson1)>().await;
    check_tuple::<(RestJson1, RpcV2Cbor, RestXml)>().await;
    check_tuple::<(RestJson1, RpcV2Cbor, RestXml, AwsJson1_0)>().await;
    check_tuple::<(RestJson1, RpcV2Cbor, RestXml, AwsJson1_0, AwsJson1_1)>().await;
    check_tuple::<(RestJson1, RpcV2Cbor, RestXml, AwsJson1_0, AwsJson1_1, ())>().await;
    check_tuple::<(RestJson1, RpcV2Cbor, RestXml, AwsJson1_0, AwsJson1_1, (), ())>().await;
    check_tuple::<(RestJson1, RpcV2Cbor, RestXml, AwsJson1_0, AwsJson1_1, (), (), ())>().await;
    check_tuple::<(RestJson1, RpcV2Cbor, RestXml, AwsJson1_0, AwsJson1_1, (), (), (), ())>().await;
    check_tuple::<(
        RestJson1,
        RpcV2Cbor,
        RestXml,
        AwsJson1_0,
        AwsJson1_1,
        (),
        (),
        (),
        (),
        (),
    )>()
    .await;
}

#[tokio::test]
async fn concrete_marker_context_hooks_use_the_supplied_protocol() {
    async fn check<P>()
    where
        InternalFailureException: IntoResponse<P>,
    {
        let selected = <InternalFailureException as IntoResponse<P>>::into_response_with_protocol(
            InternalFailureException,
            &protocol::RestXml::default(),
        );
        let legacy = IntoResponse::<RestXml>::into_response(InternalFailureException);
        assert_eq!(snapshot(selected).await, snapshot(legacy).await);
    }
    check::<RestJson1>().await;
    check::<AwsJson1_0>().await;
    check::<AwsJson1_1>().await;
    check::<RestXml>().await;
    check::<RpcV2Cbor>().await;
}
