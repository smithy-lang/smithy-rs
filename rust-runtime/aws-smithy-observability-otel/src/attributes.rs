/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Utilities to transform back and forth from Smithy [Attributes] to
//! Otel [KeyValue]s.

use std::ops::Deref;

use aws_smithy_observability::attributes::{AttributeValue, Attributes};
use opentelemetry::{KeyValue, Value};

pub(crate) struct AttributesWrap(Attributes);
impl<'a> AttributesWrap {
    pub(crate) fn new(inner: Attributes) -> Self {
        Self(inner)
    }
}
impl<'a> Deref for AttributesWrap {
    type Target = Attributes;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub(crate) fn kv_from_option_attr(input: Option<&Attributes>) -> Vec<KeyValue> {
    input
        .map(|attr| AttributesWrap::new(attr.clone()))
        .unwrap_or(AttributesWrap::new(Attributes::new()))
        .into()
}

pub(crate) fn option_attr_from_kv(input: &[KeyValue]) -> Option<Attributes> {
    if input.len() == 0 {
        return None;
    }

    Some(AttributesWrap::from(input).0)
}

impl<'a> From<AttributesWrap> for Vec<KeyValue> {
    fn from(value: AttributesWrap) -> Self {
        value
            .attributes()
            .iter()
            .map(|(k, v)| {
                KeyValue::new(
                    k.clone(),
                    match v {
                        AttributeValue::LONG(val) => Value::I64(val.clone()),
                        AttributeValue::DOUBLE(val) => Value::F64(val.clone()),
                        AttributeValue::STRING(val) => Value::String(val.clone().into()),
                        AttributeValue::BOOLEAN(val) => Value::Bool(val.clone()),
                        _ => Value::String("UNSUPPORTED ATTRIBUTE VALUE TYPE".into()),
                    },
                )
            })
            .collect::<Vec<KeyValue>>()
    }
}

impl<'a> From<&[KeyValue]> for AttributesWrap {
    fn from(value: &[KeyValue]) -> Self {
        let mut attrs = Attributes::new();

        value.iter().for_each(|kv| {
            attrs.set(
                kv.key.clone().into(),
                match &kv.value {
                    Value::Bool(val) => AttributeValue::BOOLEAN(val.clone()),
                    Value::I64(val) => AttributeValue::LONG(val.clone()),
                    Value::F64(val) => AttributeValue::DOUBLE(val.clone()),
                    Value::String(val) => AttributeValue::STRING(val.clone().into()),
                    Value::Array(_) => {
                        AttributeValue::STRING("UNSUPPORTED ATTRIBUTE VALUE TYPE".into())
                    }
                },
            )
        });

        AttributesWrap(attrs)
    }
}
