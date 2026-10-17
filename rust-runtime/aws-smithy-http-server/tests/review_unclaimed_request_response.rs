// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use aws_smithy_http_server::body::{to_boxed, Body, BoxBody};
use aws_smithy_http_server::routing::SyncRoute;
use aws_smithy_http_server::schema::routing::MultiProtocolRoutingService;
use aws_smithy_http_server::schema::{OperationSchema, SelectedOperation, ServiceSchema};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{shape_id, Schema, ShapeId, ShapeType};
use http::{Request, Response, StatusCode};
use http_body_util::BodyExt;
use std::convert::Infallible;
use tower::ServiceExt;

static UNIT: Schema<'static> = Schema::new(shape_id!("smithy.api", "Unit"), ShapeType::Structure);

// `GET /widgets/{id}`: input has only a label, so a compliant client sends no body and,
// per the `RestJsonHttpGetWithNoInput` / `RestJsonHttpPostWithNoModeledBody` protocol tests,
// MUST NOT send `Content-Type`.
static ID: Schema<'static> =
    Schema::new_member(shape_id!("test", "GetWidgetInput", "id"), ShapeType::String, "id", 0).with_http_label();

static GET_INPUT: Schema<'static> =
    Schema::new_struct(shape_id!("test", "GetWidgetInput"), ShapeType::Structure, &[&ID])
        .with_original_name("GetWidgetInput")
        .with_http(HttpTrait::new("GET", "/widgets/{id}", Some(200)));

static GET_WIDGET: OperationSchema<'static> =
    OperationSchema::new(shape_id!("test", "GetWidget"), &GET_INPUT, &UNIT, &[]);

// `POST /ping` with no input at all (synthetic input).
static PING_INPUT: Schema<'static> = Schema::new_struct(shape_id!("test", "PingInput"), ShapeType::Structure, &[])
    .with_http(HttpTrait::new("POST", "/ping", Some(200)));

static PING: OperationSchema<'static> = OperationSchema::new(shape_id!("test", "Ping"), &PING_INPUT, &UNIT, &[]);

static OPERATIONS: &[&OperationSchema<'static>] = &[&GET_WIDGET, &PING];

fn service(protocols: &'static [ShapeId<'static>]) -> &'static ServiceSchema<'static> {
    Box::leak(Box::new(ServiceSchema::new(
        shape_id!("test", "Service"),
        None,
        protocols,
        OPERATIONS,
    )))
}

fn echo_protocol() -> SyncRoute<Body> {
    SyncRoute::new(tower::service_fn(|request: Request<Body>| async move {
        let selected = SelectedOperation::get_from_request(&request).expect("selection");
        let text = format!(
            "{} {}",
            selected.protocol().protocol_id(),
            selected.operation().shape_id()
        );
        Ok::<_, Infallible>(Response::new(to_boxed(text)))
    }))
}

fn app(protocols: &'static [ShapeId<'static>]) -> MultiProtocolRoutingService {
    MultiProtocolRoutingService::builder(service(protocols))
        .operation_handler_bindings(OPERATIONS.iter().map(|op| (*op, echo_protocol())))
        .build()
        .expect("service builds")
}

async fn send(app: &MultiProtocolRoutingService, request: Request<Body>) -> (StatusCode, String) {
    let response: Response<BoxBody> = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

/// restJson1 + restXml cannot be disambiguated without `Content-Type`. Current behavior (changed
/// since the review): protocol priority applies and restJson1 claims the request, instead of the
/// earlier XML 404. The Smithy spec conflict is tracked upstream (smithy-lang/smithy#3318).
#[tokio::test]
async fn rest_json_plus_rest_xml_without_content_type_is_claimed_by_rest_json() {
    static P: &[ShapeId<'static>] = &[
        shape_id!("aws.protocols", "restJson1"),
        shape_id!("aws.protocols", "restXml"),
    ];
    let app = app(P);
    let (status, body) = send(&app, Request::get("/widgets/123").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
