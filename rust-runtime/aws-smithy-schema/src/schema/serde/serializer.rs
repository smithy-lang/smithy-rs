/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Shape serialization interfaces for the Smithy data model.

use super::error::SerdeError;
use crate::Schema;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};

/// Serializes Smithy shapes to a target format.
///
/// This trait provides a format-agnostic API for serializing the Smithy data model.
/// Implementations serialize each data type to the corresponding encoding in their
/// serial format (e.g., Smithy integers and floats to JSON numbers).
///
/// The serializer accepts a schema along with the value to provide additional
/// information about how to serialize the value (e.g., timestamp format, JSON name).
///
/// This trait is object-safe so that generated `SerializableStruct` implementations
/// can use `&mut dyn ShapeSerializer`, producing one compiled `serialize_members()`
/// per shape regardless of how many codecs exist (`shapes + codecs` rather than
/// `shapes * codecs` in binary size).
///
/// # Example
///
/// ```ignore
/// let mut serializer = JsonSerializer::new();
/// serializer.write_string(&STRING_SCHEMA, "hello")?;
/// ```
pub trait ShapeSerializer {
    /// Writes a structure to the serializer.
    ///
    /// # Arguments
    ///
    /// * `schema` - The schema of the structure being serialized
    /// * `value` - The structure to serialize
    fn write_struct(
        &mut self,
        schema: &Schema,
        value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError>;

    /// Writes a list to the serializer.
    ///
    /// # Arguments
    ///
    /// * `schema` - The schema of the list being serialized
    /// * `write_elements` - Callback that writes the list elements
    fn write_list(
        &mut self,
        schema: &Schema,
        write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError>;

    /// Writes a map to the serializer.
    ///
    /// # Arguments
    ///
    /// * `schema` - The schema of the map being serialized
    /// * `write_entries` - Callback that writes the map entries
    fn write_map(
        &mut self,
        schema: &Schema,
        write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError>;

    /// Writes a boolean value.
    fn write_boolean(&mut self, schema: &Schema, value: bool) -> Result<(), SerdeError>;

    /// Writes a byte (i8) value.
    fn write_byte(&mut self, schema: &Schema, value: i8) -> Result<(), SerdeError>;

    /// Writes a short (i16) value.
    fn write_short(&mut self, schema: &Schema, value: i16) -> Result<(), SerdeError>;

    /// Writes an integer (i32) value.
    fn write_integer(&mut self, schema: &Schema, value: i32) -> Result<(), SerdeError>;

    /// Writes a long (i64) value.
    fn write_long(&mut self, schema: &Schema, value: i64) -> Result<(), SerdeError>;

    /// Writes a float (f32) value.
    fn write_float(&mut self, schema: &Schema, value: f32) -> Result<(), SerdeError>;

    /// Writes a double (f64) value.
    fn write_double(&mut self, schema: &Schema, value: f64) -> Result<(), SerdeError>;

    /// Writes a big integer value.
    fn write_big_integer(&mut self, schema: &Schema, value: &BigInteger) -> Result<(), SerdeError>;

    /// Writes a big decimal value.
    fn write_big_decimal(&mut self, schema: &Schema, value: &BigDecimal) -> Result<(), SerdeError>;

    /// Writes a string value.
    fn write_string(&mut self, schema: &Schema, value: &str) -> Result<(), SerdeError>;

    /// Writes a blob (byte array) value.
    fn write_blob(&mut self, schema: &Schema, value: &Blob) -> Result<(), SerdeError>;

    /// Writes a timestamp value.
    fn write_timestamp(&mut self, schema: &Schema, value: &DateTime) -> Result<(), SerdeError>;

    /// Writes a document value.
    fn write_document(&mut self, schema: &Schema, value: &Document) -> Result<(), SerdeError>;

    /// Writes a null value (for sparse collections).
    fn write_null(&mut self, schema: &Schema) -> Result<(), SerdeError>;
}

/// Trait for structures that can be serialized via a schema.
///
/// Implemented by generated structure types. Because `ShapeSerializer` is object-safe,
/// each struct gets one compiled `serialize_members()` that works with any serializer
/// through dynamic dispatch.
///
/// # Example
///
/// ```ignore
/// impl SerializableStruct for MyStruct {
///     fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
///         serializer.write_string(&NAME_SCHEMA, &self.name)?;
///         serializer.write_integer(&AGE_SCHEMA, self.age)?;
///         Ok(())
///     }
/// }
/// ```
pub trait SerializableStruct {
    /// Serializes this structure's members using the provided serializer.
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError>;
}
