/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! AttributeValueConvert implementations for standard Rust types.

use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use crate::error::ConversionError;
use crate::traits::AttributeValueConvert;

// Helper to get type name from AttributeValue
fn attribute_type_name(av: &AttributeValue) -> &'static str {
    match av {
        AttributeValue::S(_) => "S",
        AttributeValue::N(_) => "N",
        AttributeValue::B(_) => "B",
        AttributeValue::Ss(_) => "SS",
        AttributeValue::Ns(_) => "NS",
        AttributeValue::Bs(_) => "BS",
        AttributeValue::M(_) => "M",
        AttributeValue::L(_) => "L",
        AttributeValue::Null(_) => "NULL",
        AttributeValue::Bool(_) => "BOOL",
        _ => "Unknown",
    }
}

// ============================================================================
// Primitive types
// ============================================================================

impl AttributeValueConvert for String {
    fn to_attribute_value(&self) -> Result<AttributeValue, ConversionError> {
        Ok(AttributeValue::S(self.clone()))
    }

    fn from_attribute_value(value: AttributeValue) -> Result<Self, ConversionError> {
        match value {
            AttributeValue::S(s) => Ok(s),
            other => Err(ConversionError::type_mismatch(
                "S",
                attribute_type_name(&other),
            )),
        }
    }
}

impl AttributeValueConvert for i64 {
    fn to_attribute_value(&self) -> Result<AttributeValue, ConversionError> {
        Ok(AttributeValue::N(self.to_string()))
    }

    fn from_attribute_value(value: AttributeValue) -> Result<Self, ConversionError> {
        match value {
            AttributeValue::N(n) => n.parse().map_err(|_| {
                ConversionError::invalid_value("", format!("cannot parse '{}' as i64", n))
            }),
            other => Err(ConversionError::type_mismatch(
                "N",
                attribute_type_name(&other),
            )),
        }
    }
}

impl AttributeValueConvert for f64 {
    fn to_attribute_value(&self) -> Result<AttributeValue, ConversionError> {
        Ok(AttributeValue::N(self.to_string()))
    }

    fn from_attribute_value(value: AttributeValue) -> Result<Self, ConversionError> {
        match value {
            AttributeValue::N(n) => n.parse().map_err(|_| {
                ConversionError::invalid_value("", format!("cannot parse '{}' as f64", n))
            }),
            other => Err(ConversionError::type_mismatch(
                "N",
                attribute_type_name(&other),
            )),
        }
    }
}

impl AttributeValueConvert for bool {
    fn to_attribute_value(&self) -> Result<AttributeValue, ConversionError> {
        Ok(AttributeValue::Bool(*self))
    }

    fn from_attribute_value(value: AttributeValue) -> Result<Self, ConversionError> {
        match value {
            AttributeValue::Bool(b) => Ok(b),
            other => Err(ConversionError::type_mismatch(
                "BOOL",
                attribute_type_name(&other),
            )),
        }
    }
}

impl AttributeValueConvert for Vec<u8> {
    fn to_attribute_value(&self) -> Result<AttributeValue, ConversionError> {
        Ok(AttributeValue::B(aws_sdk_dynamodb::primitives::Blob::new(
            self.clone(),
        )))
    }

    fn from_attribute_value(value: AttributeValue) -> Result<Self, ConversionError> {
        match value {
            AttributeValue::B(b) => Ok(b.into_inner()),
            other => Err(ConversionError::type_mismatch(
                "B",
                attribute_type_name(&other),
            )),
        }
    }
}

// ============================================================================
// Collection types
// ============================================================================

impl<T: AttributeValueConvert> AttributeValueConvert for Option<T> {
    fn to_attribute_value(&self) -> Result<AttributeValue, ConversionError> {
        match self {
            Some(v) => v.to_attribute_value(),
            None => Ok(AttributeValue::Null(true)),
        }
    }

    fn from_attribute_value(value: AttributeValue) -> Result<Self, ConversionError> {
        match value {
            AttributeValue::Null(true) => Ok(None),
            other => T::from_attribute_value(other).map(Some),
        }
    }
}

impl<T: AttributeValueConvert> AttributeValueConvert for Vec<T> {
    fn to_attribute_value(&self) -> Result<AttributeValue, ConversionError> {
        let items: Result<Vec<_>, _> = self.iter().map(|v| v.to_attribute_value()).collect();
        Ok(AttributeValue::L(items?))
    }

    fn from_attribute_value(value: AttributeValue) -> Result<Self, ConversionError> {
        match value {
            AttributeValue::L(list) => list.into_iter().map(T::from_attribute_value).collect(),
            other => Err(ConversionError::type_mismatch(
                "L",
                attribute_type_name(&other),
            )),
        }
    }
}

impl<K, V> AttributeValueConvert for HashMap<K, V>
where
    K: AttributeValueConvert + Eq + Hash + ToString + From<String>,
    V: AttributeValueConvert,
{
    fn to_attribute_value(&self) -> Result<AttributeValue, ConversionError> {
        let mut map = HashMap::new();
        for (k, v) in self {
            map.insert(k.to_string(), v.to_attribute_value()?);
        }
        Ok(AttributeValue::M(map))
    }

    fn from_attribute_value(value: AttributeValue) -> Result<Self, ConversionError> {
        match value {
            AttributeValue::M(m) => {
                let mut result = HashMap::new();
                for (k, v) in m {
                    result.insert(K::from(k), V::from_attribute_value(v)?);
                }
                Ok(result)
            }
            other => Err(ConversionError::type_mismatch(
                "M",
                attribute_type_name(&other),
            )),
        }
    }
}

impl<T: AttributeValueConvert + Eq + Hash> AttributeValueConvert for HashSet<T> {
    fn to_attribute_value(&self) -> Result<AttributeValue, ConversionError> {
        let items: Result<Vec<_>, _> = self.iter().map(|v| v.to_attribute_value()).collect();
        Ok(AttributeValue::L(items?))
    }

    fn from_attribute_value(value: AttributeValue) -> Result<Self, ConversionError> {
        match value {
            AttributeValue::L(list) => list.into_iter().map(T::from_attribute_value).collect(),
            other => Err(ConversionError::type_mismatch(
                "L",
                attribute_type_name(&other),
            )),
        }
    }
}

// ============================================================================
// Tuple types (for multi-attribute keys, up to 4 elements)
// ============================================================================

impl<T1: AttributeValueConvert> AttributeValueConvert for (T1,) {
    fn to_attribute_value(&self) -> Result<AttributeValue, ConversionError> {
        Ok(AttributeValue::L(vec![self.0.to_attribute_value()?]))
    }

    fn from_attribute_value(value: AttributeValue) -> Result<Self, ConversionError> {
        match value {
            AttributeValue::L(mut list) if list.len() == 1 => {
                Ok((T1::from_attribute_value(list.remove(0))?,))
            }
            AttributeValue::L(list) => Err(ConversionError::invalid_value(
                "",
                format!("expected list of 1 element, got {}", list.len()),
            )),
            other => Err(ConversionError::type_mismatch(
                "L",
                attribute_type_name(&other),
            )),
        }
    }
}

impl<T1: AttributeValueConvert, T2: AttributeValueConvert> AttributeValueConvert for (T1, T2) {
    fn to_attribute_value(&self) -> Result<AttributeValue, ConversionError> {
        Ok(AttributeValue::L(vec![
            self.0.to_attribute_value()?,
            self.1.to_attribute_value()?,
        ]))
    }

    fn from_attribute_value(value: AttributeValue) -> Result<Self, ConversionError> {
        match value {
            AttributeValue::L(mut list) if list.len() == 2 => {
                let v2 = list.remove(1);
                let v1 = list.remove(0);
                Ok((T1::from_attribute_value(v1)?, T2::from_attribute_value(v2)?))
            }
            AttributeValue::L(list) => Err(ConversionError::invalid_value(
                "",
                format!("expected list of 2 elements, got {}", list.len()),
            )),
            other => Err(ConversionError::type_mismatch(
                "L",
                attribute_type_name(&other),
            )),
        }
    }
}

impl<T1: AttributeValueConvert, T2: AttributeValueConvert, T3: AttributeValueConvert>
    AttributeValueConvert for (T1, T2, T3)
{
    fn to_attribute_value(&self) -> Result<AttributeValue, ConversionError> {
        Ok(AttributeValue::L(vec![
            self.0.to_attribute_value()?,
            self.1.to_attribute_value()?,
            self.2.to_attribute_value()?,
        ]))
    }

    fn from_attribute_value(value: AttributeValue) -> Result<Self, ConversionError> {
        match value {
            AttributeValue::L(mut list) if list.len() == 3 => {
                let v3 = list.remove(2);
                let v2 = list.remove(1);
                let v1 = list.remove(0);
                Ok((
                    T1::from_attribute_value(v1)?,
                    T2::from_attribute_value(v2)?,
                    T3::from_attribute_value(v3)?,
                ))
            }
            AttributeValue::L(list) => Err(ConversionError::invalid_value(
                "",
                format!("expected list of 3 elements, got {}", list.len()),
            )),
            other => Err(ConversionError::type_mismatch(
                "L",
                attribute_type_name(&other),
            )),
        }
    }
}

impl<
        T1: AttributeValueConvert,
        T2: AttributeValueConvert,
        T3: AttributeValueConvert,
        T4: AttributeValueConvert,
    > AttributeValueConvert for (T1, T2, T3, T4)
{
    fn to_attribute_value(&self) -> Result<AttributeValue, ConversionError> {
        Ok(AttributeValue::L(vec![
            self.0.to_attribute_value()?,
            self.1.to_attribute_value()?,
            self.2.to_attribute_value()?,
            self.3.to_attribute_value()?,
        ]))
    }

    fn from_attribute_value(value: AttributeValue) -> Result<Self, ConversionError> {
        match value {
            AttributeValue::L(mut list) if list.len() == 4 => {
                let v4 = list.remove(3);
                let v3 = list.remove(2);
                let v2 = list.remove(1);
                let v1 = list.remove(0);
                Ok((
                    T1::from_attribute_value(v1)?,
                    T2::from_attribute_value(v2)?,
                    T3::from_attribute_value(v3)?,
                    T4::from_attribute_value(v4)?,
                ))
            }
            AttributeValue::L(list) => Err(ConversionError::invalid_value(
                "",
                format!("expected list of 4 elements, got {}", list.len()),
            )),
            other => Err(ConversionError::type_mismatch(
                "L",
                attribute_type_name(&other),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_roundtrip() {
        let original = "hello".to_string();
        let av = original.to_attribute_value().unwrap();
        let recovered = String::from_attribute_value(av).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_i64_roundtrip() {
        let original: i64 = -42;
        let av = original.to_attribute_value().unwrap();
        let recovered = i64::from_attribute_value(av).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_f64_roundtrip() {
        let original: f64 = 3.14159;
        let av = original.to_attribute_value().unwrap();
        let recovered = f64::from_attribute_value(av).unwrap();
        assert!((original - recovered).abs() < f64::EPSILON);
    }

    #[test]
    fn test_bool_roundtrip() {
        let av = true.to_attribute_value().unwrap();
        assert!(bool::from_attribute_value(av).unwrap());
    }

    #[test]
    fn test_bytes_roundtrip() {
        let original = vec![1u8, 2, 3, 4];
        let av = original.to_attribute_value().unwrap();
        let recovered = Vec::<u8>::from_attribute_value(av).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_option_some_roundtrip() {
        let original: Option<String> = Some("test".to_string());
        let av = original.to_attribute_value().unwrap();
        let recovered = Option::<String>::from_attribute_value(av).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_option_none_roundtrip() {
        let original: Option<String> = None;
        let av = original.to_attribute_value().unwrap();
        let recovered = Option::<String>::from_attribute_value(av).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_vec_roundtrip() {
        let original = vec!["a".to_string(), "b".to_string()];
        let av = original.to_attribute_value().unwrap();
        let recovered = Vec::<String>::from_attribute_value(av).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_tuple2_roundtrip() {
        let original = ("pk".to_string(), 42i64);
        let av = original.to_attribute_value().unwrap();
        let recovered = <(String, i64)>::from_attribute_value(av).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_tuple4_roundtrip() {
        let original = (
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
        );
        let av = original.to_attribute_value().unwrap();
        let recovered = <(String, String, String, String)>::from_attribute_value(av).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_type_mismatch_error() {
        let av = AttributeValue::N("42".to_string());
        let result = String::from_attribute_value(av);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("expected S"));
    }
}
