/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! XML deserializer skeleton.
//!
//! Every method body is `todo!()`. Phase 4 of the AWS REST XML schema-serde
//! plan implements these:
//!  - 4.1: cursor / scope plumbing + `read_string`
//!  - 4.2: `read_struct` with element-name dispatch
//!  - 4.3: attribute dispatch in `read_struct`
//!  - 4.4: `read_list` / `read_map` (wrapped)
//!  - 4.5: flattened collections in `read_struct`
//!  - 4.6: scalar reads
//!  - 4.7: `read_document` returns `SerdeError`
//!  - 4.8: codec wiring

use super::XmlCodecSettings;
use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
use aws_smithy_schema::Schema;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};
use std::sync::Arc;

/// XML deserializer that implements the `ShapeDeserializer` trait.
///
/// Field layout will be expanded in Phase 4 to include the cursor / scope
/// state (likely wrapping `aws_smithy_xml::decode::Document` /
/// `ScopedDecoder`) needed for element-name dispatch.
pub struct XmlDeserializer<'a> {
    #[allow(dead_code)] // used by Phase 4 to drive parsing
    input: &'a [u8],
    #[allow(dead_code)] // used by Phase 4 for timestamp / number parsing defaults
    settings: Arc<XmlCodecSettings>,
}

impl<'a> XmlDeserializer<'a> {
    /// Creates a new XML deserializer with the given settings.
    pub(crate) fn new(input: &'a [u8], settings: Arc<XmlCodecSettings>) -> Self {
        Self { input, settings }
    }
}

impl ShapeDeserializer for XmlDeserializer<'_> {
    fn read_struct(
        &mut self,
        _schema: &Schema,
        _state: &mut dyn FnMut(&Schema, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        todo!("XmlDeserializer::read_struct — implemented in Phase 4.2")
    }

    fn read_list(
        &mut self,
        _schema: &Schema,
        _state: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        todo!("XmlDeserializer::read_list — implemented in Phase 4.4")
    }

    fn read_map(
        &mut self,
        _schema: &Schema,
        _state: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        todo!("XmlDeserializer::read_map — implemented in Phase 4.4")
    }

    fn read_boolean(&mut self, _schema: &Schema) -> Result<bool, SerdeError> {
        todo!("XmlDeserializer::read_boolean — implemented in Phase 4.6")
    }

    fn read_byte(&mut self, _schema: &Schema) -> Result<i8, SerdeError> {
        todo!("XmlDeserializer::read_byte — implemented in Phase 4.6")
    }

    fn read_short(&mut self, _schema: &Schema) -> Result<i16, SerdeError> {
        todo!("XmlDeserializer::read_short — implemented in Phase 4.6")
    }

    fn read_integer(&mut self, _schema: &Schema) -> Result<i32, SerdeError> {
        todo!("XmlDeserializer::read_integer — implemented in Phase 4.6")
    }

    fn read_long(&mut self, _schema: &Schema) -> Result<i64, SerdeError> {
        todo!("XmlDeserializer::read_long — implemented in Phase 4.6")
    }

    fn read_float(&mut self, _schema: &Schema) -> Result<f32, SerdeError> {
        todo!("XmlDeserializer::read_float — implemented in Phase 4.6")
    }

    fn read_double(&mut self, _schema: &Schema) -> Result<f64, SerdeError> {
        todo!("XmlDeserializer::read_double — implemented in Phase 4.6")
    }

    fn read_big_integer(&mut self, _schema: &Schema) -> Result<BigInteger, SerdeError> {
        todo!("XmlDeserializer::read_big_integer — implemented in Phase 4.6")
    }

    fn read_big_decimal(&mut self, _schema: &Schema) -> Result<BigDecimal, SerdeError> {
        todo!("XmlDeserializer::read_big_decimal — implemented in Phase 4.6")
    }

    fn read_string(&mut self, _schema: &Schema) -> Result<String, SerdeError> {
        todo!("XmlDeserializer::read_string — implemented in Phase 4.1")
    }

    fn read_blob(&mut self, _schema: &Schema) -> Result<Blob, SerdeError> {
        todo!("XmlDeserializer::read_blob — implemented in Phase 4.6")
    }

    fn read_timestamp(&mut self, _schema: &Schema) -> Result<DateTime, SerdeError> {
        todo!("XmlDeserializer::read_timestamp — implemented in Phase 4.6")
    }

    fn read_document(&mut self, _schema: &Schema) -> Result<Document, SerdeError> {
        todo!("XmlDeserializer::read_document — returns SerdeError in Phase 4.7 (REST XML does not support documents)")
    }

    fn is_null(&self) -> bool {
        // XML has no notion of explicit null at the element level; absence of
        // an element is what indicates "missing". A definitive answer here
        // requires the cursor/scope plumbing landing in Phase 4.1.
        todo!("XmlDeserializer::is_null — implemented in Phase 4.1")
    }

    fn container_size(&self) -> Option<usize> {
        // XML doesn't pre-declare container sizes (no length prefix as in CBOR
        // or pre-scan as in JSON). Returning `None` is the natural answer once
        // the type is wired up; for now match the pattern used elsewhere.
        None
    }
}
