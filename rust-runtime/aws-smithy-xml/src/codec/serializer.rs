/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! XML serializer skeleton.
//!
//! Every method body is `todo!()`. Phase 3 of the AWS REST XML schema-serde
//! plan implements these:
//!  - 3.1: frame state machine + `write_struct` / `write_string`
//!  - 3.2: scalar member writes
//!  - 3.3: attribute writes via `xml_attribute()`
//!  - 3.4: namespace emission on root element
//!  - 3.5: list serialization (wrapped + flattened)
//!  - 3.6: map serialization (wrapped + flattened)
//!  - 3.7: `write_document` returns `SerdeError`
//!  - 3.8: `FinishSerializer::finish` and codec wiring

use super::XmlCodecSettings;
use aws_smithy_schema::codec::FinishSerializer;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::Schema;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};
use std::sync::Arc;

/// XML serializer that implements the `ShapeSerializer` trait.
///
/// State and field layout will be expanded in Phase 3 to include the frame
/// stack required for deferred start-tag closure (so attributes can be
/// emitted inline before child elements).
pub struct XmlSerializer {
    #[allow(dead_code)] // used by Phase 3 to drive emission
    output: String,
    #[allow(dead_code)] // used by Phase 3 for timestamp formatting
    settings: Arc<XmlCodecSettings>,
}

impl XmlSerializer {
    /// Creates a new XML serializer with the given settings.
    pub(crate) fn new(settings: Arc<XmlCodecSettings>) -> Self {
        Self {
            output: String::new(),
            settings,
        }
    }
}

impl FinishSerializer for XmlSerializer {
    fn finish(self) -> Vec<u8> {
        todo!("XmlSerializer::finish — implemented in Phase 3.8")
    }
}

impl ShapeSerializer for XmlSerializer {
    fn write_struct(
        &mut self,
        _schema: &Schema,
        _value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_struct — implemented in Phase 3.1")
    }

    fn write_list(
        &mut self,
        _schema: &Schema,
        _write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_list — implemented in Phase 3.5")
    }

    fn write_map(
        &mut self,
        _schema: &Schema,
        _write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_map — implemented in Phase 3.6")
    }

    fn write_boolean(&mut self, _schema: &Schema, _value: bool) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_boolean — implemented in Phase 3.2")
    }

    fn write_byte(&mut self, _schema: &Schema, _value: i8) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_byte — implemented in Phase 3.2")
    }

    fn write_short(&mut self, _schema: &Schema, _value: i16) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_short — implemented in Phase 3.2")
    }

    fn write_integer(&mut self, _schema: &Schema, _value: i32) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_integer — implemented in Phase 3.2")
    }

    fn write_long(&mut self, _schema: &Schema, _value: i64) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_long — implemented in Phase 3.2")
    }

    fn write_float(&mut self, _schema: &Schema, _value: f32) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_float — implemented in Phase 3.2")
    }

    fn write_double(&mut self, _schema: &Schema, _value: f64) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_double — implemented in Phase 3.2")
    }

    fn write_big_integer(
        &mut self,
        _schema: &Schema,
        _value: &BigInteger,
    ) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_big_integer — implemented in Phase 3.2")
    }

    fn write_big_decimal(
        &mut self,
        _schema: &Schema,
        _value: &BigDecimal,
    ) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_big_decimal — implemented in Phase 3.2")
    }

    fn write_string(&mut self, _schema: &Schema, _value: &str) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_string — implemented in Phase 3.1")
    }

    fn write_blob(&mut self, _schema: &Schema, _value: &Blob) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_blob — implemented in Phase 3.2")
    }

    fn write_timestamp(&mut self, _schema: &Schema, _value: &DateTime) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_timestamp — implemented in Phase 3.2")
    }

    fn write_document(&mut self, _schema: &Schema, _value: &Document) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_document — returns SerdeError in Phase 3.7 (REST XML does not support documents)")
    }

    fn write_null(&mut self, _schema: &Schema) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_null — implemented in Phase 3.2")
    }
}
