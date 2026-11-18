/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Checks generated Rust API compatibility: operation IDs use the server crate's
//! ShapeId type, and a schema service supports adding a layer after construction
//! and boxing the service. Protocol-specific extractors retain their accepted and
//! rejected responses. Compilation and HTTP requests exercise those APIs.
//!
//! Review finding: `ShapeId` exists in both `aws-smithy-http-server` and `aws-smithy-schema`.
pub use protocol_compat::kit::{from_bytes, Body};
mod support;

use std::any::TypeId;

use aws_smithy_http_server::operation::OperationShape;
use compat_rest_server_sdk::operation_shape::Ping;

fn type_id_of<T: 'static>(_: &T) -> TypeId {
    TypeId::of::<T>()
}

/// Pins today's duplication so the convergence into `aws-smithy-schema` shows up as a test change.
#[test]
fn generated_operation_ids_use_the_server_crate_shape_id_not_the_schema_one() {
    let id = <Ping as OperationShape>::ID;
    assert_eq!(
        type_id_of(&id),
        TypeId::of::<aws_smithy_http_server::shape_id::ShapeId>()
    );
    assert_ne!(
        TypeId::of::<aws_smithy_http_server::shape_id::ShapeId>(),
        TypeId::of::<aws_smithy_schema::ShapeId<'static>>()
    );
}

/// The schema service keeps mainline's post-build `layer` and generic `boxed::<B>()`.
#[tokio::test]
async fn schema_service_supports_post_build_layer_and_boxed() {
    use aws_smithy_http_server::body::BoxBody;
    use aws_smithy_http_server::routing::SyncRoute;
    use protocol_compat::kit::{from_bytes, Body};
    use tower::ServiceExt;

    let app = protocol_compat::rest::service();
    #[allow(deprecated)]
    let layered = app.layer(&tower::layer::util::Identity::new());
    let boxed: compat_rest_server_sdk::ReviewRest<SyncRoute<Body>> = layered.boxed::<Body>();
    let request = http::Request::get("/widgets/123")
        .body(from_bytes(b""))
        .unwrap();
    let response: http::Response<BoxBody> = boxed.oneshot(request).await.expect("infallible");
    assert_eq!(response.status(), 200);
}

/// A sole-protocol service's protocol type is that protocol, so a `FromParts<RestJson1>` extractor
/// keeps working and its `IntoResponse<RestJson1>` rejection is what the client receives.
#[tokio::test]
async fn sole_protocol_service_accepts_protocol_specific_extractors() {
    use support::{rest, send};
    let ok = send(
        protocol_compat::caller::service(),
        rest("GET", "/widgets/1", &[("x-caller", "alice")], b""),
    )
    .await;
    assert_eq!(
        (ok.status, ok.text().as_str()),
        (200, r#"{"id":"alice:1"}"#)
    );

    let rejected = send(
        protocol_compat::caller::service(),
        rest("GET", "/widgets/1", &[], b""),
    )
    .await;
    assert_eq!(rejected.status, 401);
    assert_eq!(rejected.header("x-amzn-errortype"), Some("MissingCaller"));
    assert_eq!(rejected.text(), r#"{"message":"missing x-caller"}"#);
}
