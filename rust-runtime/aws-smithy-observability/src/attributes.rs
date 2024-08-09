/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::collections::HashMap;
// use std::marker::PhantomData;

// Helper type aliases to stay aligned with the type names in the spec
pub(crate) type Long = i64;
pub(crate) type Double = f64;

// pub(crate) struct AttributeKey<T: AttributeValueType> {
//     name: String,
//     // PhantomData to keep the key tied to the type
//     _attr_type: PhantomData<T>,
// }

/// Marker trait for allowed Attribute value types
pub(crate) trait AttributeValueType {
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

#[non_exhaustive]
pub(crate) enum AttributeValue {
    LONG(Long),
    DOUBLE(Double),
    STRING(String),
    BOOLEAN(bool),
}

pub(crate) struct AttributeMap {
    attrs: HashMap<String, AttributeValue>,
}

pub(crate) trait Attributes {
    fn set(&mut self, key: String, value: AttributeValue);
    fn get(&self, key: String) -> Option<&AttributeValue>;
}

impl Attributes for AttributeMap {
    fn set(&mut self, key: String, value: AttributeValue) {
        self.attrs.insert(key, value);
    }

    fn get(&self, key: String) -> Option<&AttributeValue> {
        self.attrs.get(&key)
    }
}

/// Delineates a logical scope that has some beginning and end
/// (e.g. a function or block of code ).
pub(crate) trait Scope {
    /// invoke when the scope has ended
    fn end(&self);
}

pub(crate) trait Context {
    /// Make this context the currently active context .
    /// The returned handle is used to return the previous
    /// context (if one existed) as active.
    fn make_current(&self) -> &dyn Scope;
}

pub(crate) trait ContextManager {
    ///Get the currently active context
    fn current(&self) -> impl Context;
}
