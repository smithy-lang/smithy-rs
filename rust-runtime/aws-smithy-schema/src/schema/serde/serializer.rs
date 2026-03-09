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
/// # Type Parameter
///
/// * `Output` - The serialization target type (e.g., `Vec<u8>`, `String`)
///
/// # Example
///
/// ```ignore
/// let mut serializer = JsonSerializer::new();
/// serializer.write_string(&STRING_SCHEMA, "hello")?;
/// let json_bytes = serializer.finish()?;
/// ```
pub trait ShapeSerializer {
    /// The serialization target type (e.g., `Vec<u8>`, `String`).
    type Output;

    /// Finalizes the serialization and returns the serialized output.
    fn finish(self) -> Result<Self::Output, SerdeError>;

    /// Writes a structure to the serializer.
    ///
    /// The structure serialization is driven by a callback that writes each member.
    /// This avoids the need for trait objects while maintaining flexibility.
    ///
    /// # Arguments
    ///
    /// * `schema` - The schema of the structure being serialized
    /// * `write_members` - Callback that writes the structure's members
    fn write_struct<F>(&mut self, schema: &dyn Schema, write_members: F) -> Result<(), SerdeError>
    where
        F: FnOnce(&mut Self) -> Result<(), SerdeError>;

    /// Writes a list to the serializer.
    ///
    /// The list serialization is driven by a callback that writes each element.
    ///
    /// # Arguments
    ///
    /// * `schema` - The schema of the list being serialized
    /// * `write_elements` - Callback that writes the list elements
    fn write_list<F>(&mut self, schema: &dyn Schema, write_elements: F) -> Result<(), SerdeError>
    where
        F: FnOnce(&mut Self) -> Result<(), SerdeError>;

    /// Writes a map to the serializer.
    ///
    /// The map serialization is driven by a callback that writes each entry.
    ///
    /// # Arguments
    ///
    /// * `schema` - The schema of the map being serialized
    /// * `write_entries` - Callback that writes the map entries
    fn write_map<F>(&mut self, schema: &dyn Schema, write_entries: F) -> Result<(), SerdeError>
    where
        F: FnOnce(&mut Self) -> Result<(), SerdeError>;

    /// Writes a boolean value.
    fn write_boolean(&mut self, schema: &dyn Schema, value: bool) -> Result<(), SerdeError>;

    /// Writes a byte (i8) value.
    fn write_byte(&mut self, schema: &dyn Schema, value: i8) -> Result<(), SerdeError>;

    /// Writes a short (i16) value.
    fn write_short(&mut self, schema: &dyn Schema, value: i16) -> Result<(), SerdeError>;

    /// Writes an integer (i32) value.
    fn write_integer(&mut self, schema: &dyn Schema, value: i32) -> Result<(), SerdeError>;

    /// Writes a long (i64) value.
    fn write_long(&mut self, schema: &dyn Schema, value: i64) -> Result<(), SerdeError>;

    /// Writes a float (f32) value.
    fn write_float(&mut self, schema: &dyn Schema, value: f32) -> Result<(), SerdeError>;

    /// Writes a double (f64) value.
    fn write_double(&mut self, schema: &dyn Schema, value: f64) -> Result<(), SerdeError>;

    /// Writes a big integer value.
    fn write_big_integer(
        &mut self,
        schema: &dyn Schema,
        value: &BigInteger,
    ) -> Result<(), SerdeError>;

    /// Writes a big decimal value.
    fn write_big_decimal(
        &mut self,
        schema: &dyn Schema,
        value: &BigDecimal,
    ) -> Result<(), SerdeError>;

    /// Writes a string value.
    fn write_string(&mut self, schema: &dyn Schema, value: &str) -> Result<(), SerdeError>;

    /// Writes a blob (byte array) value.
    fn write_blob(&mut self, schema: &dyn Schema, value: &Blob) -> Result<(), SerdeError>;

    /// Writes a timestamp value.
    fn write_timestamp(&mut self, schema: &dyn Schema, value: &DateTime) -> Result<(), SerdeError>;

    /// Writes a document value.
    fn write_document(&mut self, schema: &dyn Schema, value: &Document) -> Result<(), SerdeError>;

    /// Writes a null value (for sparse collections).
    fn write_null(&mut self, schema: &dyn Schema) -> Result<(), SerdeError>;
}

/// Trait for structures that can be serialized.
///
/// This trait is implemented by generated structure types to enable
/// schema-based serialization.
pub trait SerializableStruct {
    /// Serializes this structure using the provided serializer.
    fn serialize<S: ShapeSerializer>(&self, serializer: &mut S) -> Result<(), SerdeError>;
}
