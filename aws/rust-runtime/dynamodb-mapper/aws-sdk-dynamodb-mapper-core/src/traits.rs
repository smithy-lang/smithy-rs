/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Core traits for the DynamoDB Mapper.

use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

use crate::error::ConversionError;

/// Converts individual Rust values to/from DynamoDB AttributeValues.
///
/// This trait handles conversion of single values like `String`, `i64`, `Vec<T>`, etc.
/// For converting complete structs to/from DynamoDB items, see [`ItemConverter`].
pub trait AttributeValueConvert: Clone + Send + Sync + 'static {
    /// Converts this value to a DynamoDB AttributeValue.
    fn to_attribute_value(&self) -> Result<AttributeValue, ConversionError>;

    /// Constructs a value from a DynamoDB AttributeValue.
    fn from_attribute_value(value: AttributeValue) -> Result<Self, ConversionError>;
}

/// Converts complete Rust structs to/from DynamoDB items.
///
/// A DynamoDB item is represented as `HashMap<String, AttributeValue>` where
/// keys are attribute names and values are the serialized field values.
pub trait ItemConverter<T> {
    /// Converts this object to a DynamoDB item.
    fn to_item(&self) -> HashMap<String, AttributeValue>;

    /// Constructs an object from a DynamoDB item.
    fn from_item(item: HashMap<String, AttributeValue>) -> Result<T, ConversionError>;
}

/// Defines the DynamoDB key structure for a table or index.
///
/// This trait provides metadata about partition and sort keys, including
/// their Rust types and DynamoDB attribute names.
pub trait ItemSchema<T: Sized> {
    /// Partition key type - single type or tuple for multi-attribute keys.
    type PartitionKey: AttributeValueConvert;

    /// Sort key type - [`NoSortKey`] for partition-only tables, single type,
    /// or tuple for multi-attribute sort keys.
    type SortKey: AttributeValueConvert;

    /// Returns partition key attribute names in order.
    fn partition_key_names(&self) -> &'static [&'static str];

    /// Returns sort key attribute names in order, or `None` for partition-only tables.
    fn sort_key_names(&self) -> Option<&'static [&'static str]>;
}

/// Links a Rust type to its DynamoDB schema implementation.
///
/// This trait is the entry point for schema discovery. The mapper uses this
/// to obtain schema metadata (key structure, attribute names) for any type.
pub trait ProvideItemSchema: Sized {
    /// The schema type that provides metadata for this item type.
    type Schema: ItemSchema<Self>;

    /// Returns the schema instance for this type.
    fn schema() -> Self::Schema;
}

/// Extracts key values from an object without full item conversion.
///
/// This trait enables efficient key-only operations by extracting just the
/// partition and sort key values. Unlike [`ItemConverter::to_item`], this
/// avoids serializing non-key fields.
pub trait ExtractKey {
    /// The partition key type (must match the schema's partition key type).
    type PartitionKey: AttributeValueConvert;

    /// The sort key type (must match the schema's sort key type).
    type SortKey: AttributeValueConvert;

    /// Extracts the partition key value from this object.
    fn extract_partition_key(&self) -> Self::PartitionKey;

    /// Extracts the sort key value from this object.
    /// Returns `None` for partition-only tables.
    fn extract_sort_key(&self) -> Option<Self::SortKey> {
        None
    }
}

/// Marker type for tables without a sort key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NoSortKey;

impl AttributeValueConvert for NoSortKey {
    fn to_attribute_value(&self) -> Result<AttributeValue, ConversionError> {
        Ok(AttributeValue::Null(true))
    }

    fn from_attribute_value(_value: AttributeValue) -> Result<Self, ConversionError> {
        Ok(NoSortKey)
    }
}
