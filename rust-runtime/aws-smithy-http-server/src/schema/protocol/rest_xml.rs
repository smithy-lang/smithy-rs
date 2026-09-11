/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, ShapeId};
use aws_smithy_xml::codec::XmlCodec;

use crate::protocol::rest_xml::rejection::RequestRejection;
use crate::protocol::rest_xml::runtime_error::RuntimeError;
use crate::protocol::rest_xml::{RestXml, RestXmlProtocol};
use crate::response::{IntoResponse, Response};
use crate::schema::{DeserializeError, HttpModeledError};

use super::response::{
    log_serialize_failure, serialize_modeled_error_response, stamp_error_extension, ResponseBindings,
};
use super::rest::RestProtocolProvider;
use super::{CompiledOperation, RestOperationState, ServerProtocol, ServerRequest};

static PROTOCOL_ID: ShapeId<'static> = shape_id!("aws.protocols", "restXml");
const CONTENT_TYPE: &str = "application/xml";

impl RestProtocolProvider for RestXmlProtocol {
    type RestCodec = XmlCodec;

    fn rest_protocol(&self) -> &super::rest::RestProtocol<XmlCodec> {
        &self.inner
    }
}

impl ServerProtocol for RestXmlProtocol {
    type Codec = XmlCodec;
    type OperationState = RestOperationState;

    fn protocol_id(&self) -> &'static ShapeId<'static> {
        &PROTOCOL_ID
    }

    fn codec(&self) -> &XmlCodec {
        self.inner.codec()
    }

    fn reads_request_body(&self, operation: &CompiledOperation<RestOperationState>) -> bool {
        operation.state().reads_body()
    }

    fn deserialize_request<'a>(
        &'a self,
        operation: &'a CompiledOperation<RestOperationState>,
        request: &'a ServerRequest,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
        self.inner.deserialize_request(operation.state(), request)
    }

    fn serialize_response(
        &self,
        operation: &CompiledOperation<RestOperationState>,
        output: &dyn SerializableStruct,
    ) -> Response {
        self.inner
            .serialize_response(operation.schema(), operation.state(), output)
            .unwrap_or_else(serialization_failure)
    }

    fn serialize_error(&self, error: &dyn HttpModeledError) -> Response {
        // restXml carries no discriminator: the error structure is the body.
        let schema = error.schema();
        serialize_modeled_error_response(
            self.codec(),
            schema,
            error,
            error.status_code(),
            ResponseBindings::Rest,
            CONTENT_TYPE,
        )
        .map(|response| stamp_error_extension(response, schema.shape_id().shape_name()))
        .unwrap_or_else(serialization_failure)
    }

    /// restXml keeps 415 for `Content-Type` failures, but its `From<RequestRejection>` has no
    /// `NotAcceptable` arm, so an `Accept` mismatch falls through to a 400 `Serialization` — NOT
    /// a 406. And its `RuntimeError` response drops the validation body entirely: every runtime
    /// error, constraint violations included, answers with the literal `{}` body, so a constraint
    /// violation must NOT serialize the modeled error here. Both are the protocol's wire contract.
    fn serialize_rejection(&self, err: DeserializeError) -> Response {
        match err {
            DeserializeError::Serde(err) => {
                IntoResponse::<RestXml>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
            }
            DeserializeError::UnsupportedMediaType(reason) => IntoResponse::<RestXml>::into_response(
                RuntimeError::from(RequestRejection::MissingContentType(*reason)),
            ),
            DeserializeError::NotAcceptable => {
                IntoResponse::<RestXml>::into_response(RuntimeError::from(RequestRejection::NotAcceptable))
            }
            // The smuggled reason string never reaches the wire; legacy renders `{}` regardless.
            DeserializeError::ConstraintViolation(_) => {
                IntoResponse::<RestXml>::into_response(RuntimeError::Validation(String::new()))
            }
        }
    }
}

fn serialization_failure(err: SerdeError) -> Response {
    log_serialize_failure(&err);
    IntoResponse::<RestXml>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
}
