/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Shape deserialization interfaces for the Smithy data model.

use super::error::SerdeError;
use crate::Schema;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};

/// Deserializes Smithy shapes from a serial format.
///
/// This trait provides a format-agnostic API for deserializing the Smithy data model.
/// Implementations read from a serial format and create data objects based on schemas.
///
/// The deserializer uses a consumer pattern for aggregate types (structures, lists, maps)
/// to avoid trait object limitations and enable efficient deserialization without
/// intermediate allocations.
///
/// # Consumer Pattern
///
/// For aggregate types, the deserializer calls a consumer function for each element/member.
/// The consumer receives mutable state and updates it with each deserialized value.
/// This pattern:
/// - Avoids trait object issues with generic methods
/// - Enables zero-cost abstractions (closures can be inlined)
/// - Allows caller to control deserialization order and state management
/// - Matches the SEP's recommendation for compiled typed languages
/// - Uses `&mut dyn ShapeDeserializer` so composite deserializers (e.g., HTTP
///   binding + body) can transparently delegate without the consumer knowing
///   the concrete deserializer type. This enables runtime protocol swapping.
///
/// # Example
///
/// ```ignore
/// // Deserializing a structure
/// let mut builder = MyStructBuilder::default();
/// deserializer.read_struct(
///     &MY_STRUCT_SCHEMA,
///     &mut |member, deser| {
///         match member.member_index() {
///             Some(0) => builder.field1 = Some(deser.read_string(member)?),
///             Some(1) => builder.field2 = Some(deser.read_integer(member)?),
///             _ => {}
///         }
///         Ok(())
///     },
/// )?;
/// let my_struct = builder.build();
/// ```
pub trait ShapeDeserializer {
    /// Reads a structure from the deserializer.
    ///
    /// The consumer is called for each member with the member schema and a
    /// `&mut dyn ShapeDeserializer` to read the member value. Using `dyn`
    /// allows composite deserializers (e.g., HTTP binding + body) to
    /// transparently delegate without the consumer knowing the concrete type.
    fn read_struct(
        &mut self,
        schema: &Schema,
        state: &mut dyn FnMut(&Schema, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError>;

    /// Reads a list from the deserializer.
    ///
    /// The consumer is called for each element with a `&mut dyn ShapeDeserializer`.
    fn read_list(
        &mut self,
        schema: &Schema,
        state: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError>;

    /// Reads a map from the deserializer.
    ///
    /// The consumer is called for each entry with the key and a `&mut dyn ShapeDeserializer`.
    fn read_map(
        &mut self,
        schema: &Schema,
        state: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError>;

    /// Reads a boolean value.
    fn read_boolean(&mut self, schema: &Schema) -> Result<bool, SerdeError>;

    /// Reads a byte (i8) value.
    fn read_byte(&mut self, schema: &Schema) -> Result<i8, SerdeError>;

    /// Reads a short (i16) value.
    fn read_short(&mut self, schema: &Schema) -> Result<i16, SerdeError>;

    /// Reads an integer (i32) value.
    fn read_integer(&mut self, schema: &Schema) -> Result<i32, SerdeError>;

    /// Reads a long (i64) value.
    fn read_long(&mut self, schema: &Schema) -> Result<i64, SerdeError>;

    /// Reads a float (f32) value.
    fn read_float(&mut self, schema: &Schema) -> Result<f32, SerdeError>;

    /// Reads a double (f64) value.
    fn read_double(&mut self, schema: &Schema) -> Result<f64, SerdeError>;

    /// Reads a big integer value.
    fn read_big_integer(&mut self, schema: &Schema) -> Result<BigInteger, SerdeError>;

    /// Reads a big decimal value.
    fn read_big_decimal(&mut self, schema: &Schema) -> Result<BigDecimal, SerdeError>;

    /// Reads a string value.
    fn read_string(&mut self, schema: &Schema) -> Result<String, SerdeError>;

    /// Reads a blob (byte array) value.
    fn read_blob(&mut self, schema: &Schema) -> Result<Blob, SerdeError>;

    /// Reads a timestamp value.
    fn read_timestamp(&mut self, schema: &Schema) -> Result<DateTime, SerdeError>;

    /// Reads a document value.
    fn read_document(&mut self, schema: &Schema) -> Result<Document, SerdeError>;

    /// Checks if the current value is null.
    ///
    /// This is used for sparse collections where null values are significant.
    fn is_null(&self) -> bool;

    /// Returns the size of the current container if known.
    ///
    /// This is an optimization hint that allows pre-allocating collections
    /// with the correct capacity. Returns `None` if the size is unknown or
    /// not applicable.
    ///
    /// Implementations MUST cap the returned value at a reasonable maximum
    /// (e.g. 10,000) to prevent denial-of-service from untrusted payloads
    /// that claim excessively large container sizes (e.g. a CBOR header
    /// declaring billions of elements).
    fn container_size(&self) -> Option<usize>;
}
