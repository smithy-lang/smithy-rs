/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Shape deserialization interfaces for the Smithy data model.

use crate::Schema;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};
use std::error::Error;

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
///
/// # Example
///
/// ```ignore
/// // Deserializing a structure
/// let mut builder = MyStructBuilder::default();
/// deserializer.read_struct(
///     &MY_STRUCT_SCHEMA,
///     builder,
///     |mut builder, member, deser| {
///         match member.member_index() {
///             0 => builder.field1 = Some(deser.read_string(member)?),
///             1 => builder.field2 = Some(deser.read_i32(member)?),
///             _ => {}
///         }
///         Ok(builder)
///     },
/// )?;
/// let my_struct = builder.build();
/// ```
pub trait ShapeDeserializer {
    /// The error type returned by deserialization operations.
    type Error: Error;

    /// Reads a structure from the deserializer.
    ///
    /// The structure deserialization is driven by a consumer callback that is called
    /// for each member. The consumer receives the current state, the member schema,
    /// and the deserializer, and returns the updated state.
    ///
    /// # Arguments
    ///
    /// * `schema` - The schema of the structure being deserialized
    /// * `state` - Initial state (typically a builder)
    /// * `consumer` - Callback invoked for each member with (state, member_schema, deserializer)
    ///
    /// # Returns
    ///
    /// The final state after processing all members
    fn read_struct<T, F>(
        &mut self,
        schema: &dyn Schema,
        state: T,
        consumer: F,
    ) -> Result<T, Self::Error>
    where
        F: FnMut(T, &dyn Schema, &mut Self) -> Result<T, Self::Error>;

    /// Reads a list from the deserializer.
    ///
    /// The list deserialization is driven by a consumer callback that is called
    /// for each element. The consumer receives the current state and the deserializer,
    /// and returns the updated state.
    ///
    /// # Arguments
    ///
    /// * `schema` - The schema of the list being deserialized
    /// * `state` - Initial state (typically a Vec or collection)
    /// * `consumer` - Callback invoked for each element with (state, deserializer)
    ///
    /// # Returns
    ///
    /// The final state after processing all elements
    fn read_list<T, F>(
        &mut self,
        schema: &dyn Schema,
        state: T,
        consumer: F,
    ) -> Result<T, Self::Error>
    where
        F: FnMut(T, &mut Self) -> Result<T, Self::Error>;

    /// Reads a map from the deserializer.
    ///
    /// The map deserialization is driven by a consumer callback that is called
    /// for each entry. The consumer receives the current state, the key, and the
    /// deserializer, and returns the updated state.
    ///
    /// # Arguments
    ///
    /// * `schema` - The schema of the map being deserialized
    /// * `state` - Initial state (typically a HashMap or collection)
    /// * `consumer` - Callback invoked for each entry with (state, key, deserializer)
    ///
    /// # Returns
    ///
    /// The final state after processing all entries
    fn read_map<T, F>(
        &mut self,
        schema: &dyn Schema,
        state: T,
        consumer: F,
    ) -> Result<T, Self::Error>
    where
        F: FnMut(T, String, &mut Self) -> Result<T, Self::Error>;

    /// Reads a boolean value.
    fn read_boolean(&mut self, schema: &dyn Schema) -> Result<bool, Self::Error>;

    /// Reads a byte (i8) value.
    fn read_byte(&mut self, schema: &dyn Schema) -> Result<i8, Self::Error>;

    /// Reads a short (i16) value.
    fn read_short(&mut self, schema: &dyn Schema) -> Result<i16, Self::Error>;

    /// Reads an integer (i32) value.
    fn read_integer(&mut self, schema: &dyn Schema) -> Result<i32, Self::Error>;

    /// Reads a long (i64) value.
    fn read_long(&mut self, schema: &dyn Schema) -> Result<i64, Self::Error>;

    /// Reads a float (f32) value.
    fn read_float(&mut self, schema: &dyn Schema) -> Result<f32, Self::Error>;

    /// Reads a double (f64) value.
    fn read_double(&mut self, schema: &dyn Schema) -> Result<f64, Self::Error>;

    /// Reads a big integer value.
    fn read_big_integer(&mut self, schema: &dyn Schema) -> Result<BigInteger, Self::Error>;

    /// Reads a big decimal value.
    fn read_big_decimal(&mut self, schema: &dyn Schema) -> Result<BigDecimal, Self::Error>;

    /// Reads a string value.
    fn read_string(&mut self, schema: &dyn Schema) -> Result<String, Self::Error>;

    /// Reads a blob (byte array) value.
    fn read_blob(&mut self, schema: &dyn Schema) -> Result<Blob, Self::Error>;

    /// Reads a timestamp value.
    fn read_timestamp(&mut self, schema: &dyn Schema) -> Result<DateTime, Self::Error>;

    /// Reads a document value.
    fn read_document(&mut self, schema: &dyn Schema) -> Result<Document, Self::Error>;

    /// Checks if the current value is null.
    ///
    /// This is used for sparse collections where null values are significant.
    fn is_null(&self) -> bool;

    /// Returns the size of the current container if known.
    ///
    /// This is an optimization hint that allows pre-allocating collections
    /// with the correct capacity. Returns `None` if the size is unknown or
    /// not applicable.
    fn container_size(&self) -> Option<usize>;
}
