/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http_server::{
    body::to_boxed,
    rejection::any_rejections,
    request::FromParts,
    response::{IntoResponse, Response},
    schema::protocol::{RestJson1, RpcV2Cbor},
};
use http_body_util::BodyExt;

struct LegacyRejection;

// An existing downstream implementation supplies only the original required method.
impl<P> IntoResponse<P> for LegacyRejection {
    fn into_response(self) -> Response {
        let mut response = Response::builder()
            .status(401)
            .header("x-custom-rejection", "preserved")
            .body(to_boxed("legacy rejection"))
            .unwrap();
        response.extensions_mut().insert(RejectionMarker);
        response
    }
}

#[derive(Clone)]
struct RejectionMarker;

struct LegacyExtractor;

impl<P> FromParts<P> for LegacyExtractor {
    type Rejection = LegacyRejection;

    fn from_parts(_: &mut http::request::Parts) -> Result<Self, Self::Rejection> {
        Err(LegacyRejection)
    }
}

// No additional associated-type bound: this is the generic caller C039 identifies.
fn existing_generic<P, T: FromParts<P>>(error: T::Rejection) -> Response {
    error.into_response()
}

#[tokio::test]
async fn original_implementation_and_implied_bound_remain_usable() {
    let response = existing_generic::<RestJson1, LegacyExtractor>(LegacyRejection);
    assert_eq!(response.status(), 401);
    assert_eq!(response.headers()["x-custom-rejection"], "preserved");
    assert!(response.extensions().get::<RejectionMarker>().is_some());
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "legacy rejection"
    );
}

#[tokio::test]
async fn contextual_default_preserves_complete_custom_responses_through_tuple_wrappers() {
    let rejection = any_rejections::Two::<LegacyRejection, LegacyRejection>::B(LegacyRejection);
    let response =
        IntoResponse::<(RestJson1, RpcV2Cbor)>::into_response_with_protocol(rejection, &RpcV2Cbor::default());
    assert_eq!(response.status(), 401);
    assert_eq!(response.headers()["x-custom-rejection"], "preserved");
    assert!(response.extensions().get::<RejectionMarker>().is_some());
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "legacy rejection"
    );
}
