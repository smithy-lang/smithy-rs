/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::serde::SerializableDocument;
use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;
use aws_smithy_lang::ShapeId;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::Schema;
use aws_smithy_types::Document;
use std::fmt;
use std::sync::Arc;

/// The input of one call, as interceptors see the input.
///
/// The input is created when the call is sent, after the input members are checked against the
/// operation's input schema. So an interceptor sees an input that matches the input schema. The
/// input schema is read with [`SerializableStruct::schema`].
// TODO(dynamic-client): Add the `@streaming` payload of the input.
pub struct DynamicInput {
    members: Document,
    schemas: Arc<RuntimeSchemaSet>,
    operation: ShapeId,
}

impl DynamicInput {
    pub(crate) fn new(
        members: Document,
        schemas: Arc<RuntimeSchemaSet>,
        operation: ShapeId,
    ) -> Self {
        Self {
            members,
            schemas,
            operation,
        }
    }

    /// Returns the input members, as a document object keyed by the Smithy member names of the
    /// input.
    pub fn members(&self) -> &Document {
        &self.members
    }

    /// Returns the input members, consuming the input.
    pub fn into_members(self) -> Document {
        self.members
    }
}

impl SerializableStruct for DynamicInput {
    fn schema(&self) -> &Schema<'_> {
        self.schemas
            .operation(&self.operation)
            .expect("the client found the operation")
            .input()
    }

    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        SerializableDocument::new(&self.members, self.schema(), &self.schemas)?
            .serialize_members(serializer)
    }
}

// The debug output has no input values, because a value can be `@sensitive`.
impl fmt::Debug for DynamicInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DynamicInput").finish_non_exhaustive()
    }
}
