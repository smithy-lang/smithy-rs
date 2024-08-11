/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Attributes (also referred to as tags or annotations in other telemetry systems) are structured
//! key-value pairs that annotate a span or event. Structured data allows observability backends
//! to index and process telemetry data in ways that simple log messages lack.

use std::collections::HashMap;

/// Helper type aliase to stay aligned with the type names in the spec
pub type Long = i64;
/// Helper type aliase to stay aligned with the type names in the spec
pub type Double = f64;

/// Marker trait for allowed Attribute value types
pub trait AttributeValueType {
    /// Get the [AttributeValue] variant for the given type.
    fn get_attribute_variant(self) -> AttributeValue;
}

impl AttributeValueType for Long {
    fn get_attribute_variant(self) -> AttributeValue {
        AttributeValue::LONG(self)
    }
}
impl AttributeValueType for Double {
    fn get_attribute_variant(self) -> AttributeValue {
        AttributeValue::DOUBLE(self)
    }
}
impl AttributeValueType for String {
    fn get_attribute_variant(self) -> AttributeValue {
        AttributeValue::STRING(self)
    }
}
impl AttributeValueType for bool {
    fn get_attribute_variant(self) -> AttributeValue {
        AttributeValue::BOOLEAN(self)
    }
}

/// The valid types of values accepted by [Attributes]
#[non_exhaustive]
pub enum AttributeValue {
    /// Holds an [i64]
    LONG(Long),
    /// Holds an [f64]
    DOUBLE(Double),
    /// Holds a [String]
    STRING(String),
    /// Holds a [bool]
    BOOLEAN(bool),
}

/// Structured telemetry metadata
pub struct Attributes {
    attrs: HashMap<String, AttributeValue>,
}

impl Attributes {
    /// Set an attribute
    pub fn set(&mut self, key: String, value: AttributeValue) {
        self.attrs.insert(key, value);
    }

    /// Get an attribute
    pub fn get(&self, key: String) -> Option<&AttributeValue> {
        self.attrs.get(&key)
    }
}

/// Delineates a logical scope that has some beginning and end
/// (e.g. a function or block of code ).
pub trait Scope {
    /// invoke when the scope has ended
    fn end(&self);
}

/// A cross cutting concern for carrying execution-scoped values across API
/// boundaries (both in-process and distributed).
pub trait Context {
    /// Make this context the currently active context .
    /// The returned handle is used to return the previous
    /// context (if one existed) as active.
    fn make_current(&self) -> &dyn Scope;
}

/// Keeps track of the current [Context].
pub trait ContextManager {
    ///Get the currently active context
    fn current(&self) -> &dyn Context;
}
