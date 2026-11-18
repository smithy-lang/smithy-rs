/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This module holds the request serializer and the response deserializer of a call.

mod document;

use crate::call::{DynamicInput, DynamicOutput};
use crate::error::DynamicOperationError;
use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;
use aws_smithy_lang::ShapeId;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::{Error, Input, Output};
use aws_smithy_runtime_api::client::orchestrator::{HttpRequest, HttpResponse, OrchestratorError};
use aws_smithy_runtime_api::client::ser_de::{DeserializeResponse, SerializeRequest};
use aws_smithy_schema::protocol::SharedClientProtocol;
use aws_smithy_types::config_bag::ConfigBag;
use document::{read_struct, SerializableDocument};
use std::sync::Arc;

// The serializer writes the input document through the protocol, by the operation's input
// schema.
#[derive(Debug)]
pub(crate) struct DocumentSerializer {
    schemas: Arc<RuntimeSchemaSet>,
    operation: ShapeId,
}

impl DocumentSerializer {
    pub(crate) fn new(schemas: Arc<RuntimeSchemaSet>, operation: ShapeId) -> Self {
        Self { schemas, operation }
    }
}

impl SerializeRequest for DocumentSerializer {
    fn serialize_input(&self, input: Input, cfg: &mut ConfigBag) -> Result<HttpRequest, BoxError> {
        let input = input
            .downcast::<DynamicInput>()
            .expect("the input of a call is a `DynamicInput`");
        let schema = self
            .schemas
            .operation(&self.operation)
            .expect("the client found the operation")
            .input();
        let protocol = cfg
            .load::<SharedClientProtocol>()
            .expect("build() stores the protocol");
        let value = SerializableDocument::new(input.members(), schema, &self.schemas)?;
        // A protocol takes the request path from the model or the config, so the path is empty.
        Ok(protocol.serialize_request(&value, schema, "", cfg)?)
    }
}

// The deserializer reads the response through the protocol, by the operation's output schema.
#[derive(Debug)]
pub(crate) struct DocumentDeserializer {
    schemas: Arc<RuntimeSchemaSet>,
    operation: ShapeId,
}

impl DocumentDeserializer {
    pub(crate) fn new(schemas: Arc<RuntimeSchemaSet>, operation: ShapeId) -> Self {
        Self { schemas, operation }
    }
}

impl DeserializeResponse for DocumentDeserializer {
    fn deserialize_nonstreaming_with_config(
        &self,
        response: &HttpResponse,
        cfg: &ConfigBag,
    ) -> Result<Output, OrchestratorError<Error>> {
        let protocol = cfg
            .load::<SharedClientProtocol>()
            .expect("build() stores the protocol");
        // TODO(dynamic-client): Also read a response with the status code of the operation's
        // `@http` trait as a success, as generated clients do.
        if !response.status().is_success() {
            let meta = protocol
                .parse_error_metadata(response, cfg)
                .map_err(OrchestratorError::other)?
                .build();
            return Err(OrchestratorError::operation(Error::erase(
                DynamicOperationError::unhandled(meta),
            )));
        }
        let schema = self
            .schemas
            .operation(&self.operation)
            .expect("the client found the operation")
            .output();
        let mut deserializer = protocol
            .deserialize_response(response, schema, cfg)
            .map_err(OrchestratorError::other)?;
        let members = read_struct(&self.schemas, schema, &mut *deserializer)
            .map_err(OrchestratorError::other)?;
        Ok(Output::erase(DynamicOutput::new(members)))
    }
}
