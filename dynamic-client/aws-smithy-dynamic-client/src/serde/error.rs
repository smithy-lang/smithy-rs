/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This module reads an error response into a modeled or unhandled `DynamicOperationError`.

use super::document::read_struct;
use crate::error::DynamicOperationError;
use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;
use aws_smithy_lang::ShapeId;
use aws_smithy_runtime_api::client::interceptors::context::Error;
use aws_smithy_runtime_api::client::orchestrator::{HttpResponse, OrchestratorError};
use aws_smithy_schema::error_envelope::sanitize_error_code;
use aws_smithy_schema::protocol::SharedClientProtocol;
use aws_smithy_schema::Schema;
use aws_smithy_types::config_bag::ConfigBag;
use aws_smithy_types::retry::ErrorKind;
use aws_smithy_types::Document;

const RETRYABLE: ShapeId = ShapeId::from_static("smithy.api#retryable");
const ERROR: ShapeId = ShapeId::from_static("smithy.api#error");

// Reads the error response of operation `operation`. An error code that matches one of the
// operation's errors, or one of the service's, gives a modeled error. Any other code gives an
// unhandled error.
pub(super) fn read_error(
    schemas: &RuntimeSchemaSet,
    operation: &ShapeId,
    protocol: &SharedClientProtocol,
    response: &HttpResponse,
    cfg: &ConfigBag,
) -> Result<DynamicOperationError, OrchestratorError<Error>> {
    let meta = protocol
        .parse_error_metadata(response, cfg)
        .map_err(OrchestratorError::other)?
        .build();
    let errors = schemas
        .operation(operation)
        .expect("the client found the operation")
        .errors();
    let Some(schema) = meta.code().and_then(|code| find_error(errors, code)) else {
        return Ok(DynamicOperationError::unhandled(meta));
    };
    let mut deserializer = protocol
        .deserialize_error_response(response, cfg)
        .map_err(OrchestratorError::other)?;
    let mut members =
        read_struct(schemas, schema, &mut *deserializer).map_err(OrchestratorError::other)?;
    // A generated error takes its message from the error metadata when the body has none.
    if let (Some(message), Document::Object(members)) = (meta.message(), &mut members) {
        if let Some(name) = message_member(schema) {
            members
                .entry(name.to_owned())
                .or_insert_with(|| Document::String(message.to_owned()));
        }
    }
    let shape_id =
        ShapeId::new(schema.shape_id().as_str()).expect("runtime schemas have valid shape IDs");
    let retryable = retryable_kind(schemas, &shape_id);
    Ok(DynamicOperationError::modeled(
        shape_id, members, retryable, meta,
    ))
}

// Finds the error schema whose shape name matches the wire error `code`. A `namespace#` prefix
// and a `:url` suffix are dropped first, so an absolute code and a relative code both match.
// TODO(aws-smithy-schema): Look the error up with `TypeRegistry::entry_for_error_code`, once a
// registry can hold runtime-built schemas and deserializers.
// TODO(dynamic-client): Also match the code of an error's `@awsQueryError` trait, for awsQuery
// and awsQueryCompatible services. That code can differ from the shape name.
fn find_error<'s>(errors: &[&'s Schema<'s>], code: &str) -> Option<&'s Schema<'s>> {
    let code = sanitize_error_code(code);
    errors
        .iter()
        .copied()
        .find(|schema| schema.shape_id().shape_name() == code)
}

// Returns the name of the error's message member, as generated clients find it. The message
// member is the member named `message`, in any case.
fn message_member<'s>(schema: &'s Schema<'s>) -> Option<&'s str> {
    schema
        .members()
        .iter()
        .filter_map(|member| member.member_name())
        .find(|name| name.eq_ignore_ascii_case("message"))
}

// Returns the retry kind of an error with `@retryable`, as generated clients map it. The kind is
// throttling when the trait sets `throttling`, and otherwise the error's client or server fault.
// TODO(aws-smithy-lang): Read `@retryable` and `@error` from the error schema, once runtime
// schemas carry every model trait.
fn retryable_kind(schemas: &RuntimeSchemaSet, error: &ShapeId) -> Option<ErrorKind> {
    let shape = schemas.model().get_shape(error)?;
    let retryable = shape.get_trait(&RETRYABLE)?;
    let throttling = retryable
        .as_object()
        .and_then(|value| value.get("throttling"))
        .and_then(|value| value.as_bool())
        == Some(true);
    if throttling {
        return Some(ErrorKind::ThrottlingError);
    }
    match shape.get_trait(&ERROR).and_then(|value| value.as_str()) {
        Some("server") => Some(ErrorKind::ServerError),
        _ => Some(ErrorKind::ClientError),
    }
}
