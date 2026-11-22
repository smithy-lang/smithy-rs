/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// This module holds the value that `DynamicClient::struct_value` returns.

use crate::serde::SerializableDocument;
use aws_smithy_lang::runtime_schema::RuntimeSchemaSet;
use aws_smithy_lang::ShapeId;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::Schema;
use aws_smithy_types::Document;
use std::sync::Arc;

// `StructValue` holds a structure or union value and the shape it was checked against. The value
// was checked when it was created, so it matches the shape's schema.
pub(super) struct StructValue {
    members: Document,
    schemas: Arc<RuntimeSchemaSet>,
    shape: ShapeId,
}

impl StructValue {
    pub(super) fn new(members: Document, schemas: Arc<RuntimeSchemaSet>, shape: ShapeId) -> Self {
        Self {
            members,
            schemas,
            shape,
        }
    }
}

impl SerializableStruct for StructValue {
    fn schema(&self) -> &Schema<'_> {
        self.schemas
            .schema(&self.shape)
            .expect("the shape was found when the value was created")
    }

    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        SerializableDocument::new(&self.members, self.schema(), &self.schemas)?
            .serialize_members(serializer)
    }
}
