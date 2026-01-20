/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Error types for type conversion operations.

use std::fmt;

/// Error that occurs during type conversion between Rust types and DynamoDB AttributeValues.
#[derive(Debug)]
pub struct ConversionError {
    kind: ConversionErrorKind,
    field: Option<String>,
}

// TODO(hll): revisit pub error type

/// The kind of conversion error that occurred.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConversionErrorKind {
    /// A required attribute was missing from the item.
    MissingAttribute,
    /// The attribute value had an unexpected type.
    InvalidType {
        /// The expected DynamoDB type.
        expected: &'static str,
        /// The actual DynamoDB type found.
        actual: &'static str,
    },
    /// The attribute value could not be parsed or was invalid.
    InvalidValue {
        /// Description of why the value was invalid.
        message: String,
    },
}

impl ConversionError {
    /// Creates an error for a missing attribute.
    pub fn missing_attribute(field: impl Into<String>) -> Self {
        Self {
            kind: ConversionErrorKind::MissingAttribute,
            field: Some(field.into()),
        }
    }

    /// Creates an error for an invalid type.
    pub fn invalid_type(
        field: impl Into<String>,
        expected: &'static str,
        actual: &'static str,
    ) -> Self {
        Self {
            kind: ConversionErrorKind::InvalidType { expected, actual },
            field: Some(field.into()),
        }
    }

    /// Creates an error for an invalid value.
    pub fn invalid_value(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: ConversionErrorKind::InvalidValue {
                message: message.into(),
            },
            field: Some(field.into()),
        }
    }

    /// Creates a type error without a field name (for standalone conversions).
    pub fn type_mismatch(expected: &'static str, actual: &'static str) -> Self {
        Self {
            kind: ConversionErrorKind::InvalidType { expected, actual },
            field: None,
        }
    }

    /// Returns the kind of error.
    pub fn kind(&self) -> &ConversionErrorKind {
        &self.kind
    }

    /// Returns the field name if available.
    pub fn field(&self) -> Option<&str> {
        self.field.as_deref()
    }
}

impl fmt::Display for ConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.kind, &self.field) {
            (ConversionErrorKind::MissingAttribute, Some(field)) => {
                write!(f, "missing required attribute '{}'", field)
            }
            (ConversionErrorKind::MissingAttribute, None) => {
                write!(f, "missing required attribute")
            }
            (ConversionErrorKind::InvalidType { expected, actual }, Some(field)) => {
                write!(
                    f,
                    "invalid type for '{}': expected {}, got {}",
                    field, expected, actual
                )
            }
            (ConversionErrorKind::InvalidType { expected, actual }, None) => {
                write!(f, "invalid type: expected {}, got {}", expected, actual)
            }
            (ConversionErrorKind::InvalidValue { message }, Some(field)) => {
                write!(f, "invalid value for '{}': {}", field, message)
            }
            (ConversionErrorKind::InvalidValue { message }, None) => {
                write!(f, "invalid value: {}", message)
            }
        }
    }
}

impl std::error::Error for ConversionError {}
