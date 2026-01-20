/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Key specification types for DynamoDB operations.

use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;

use crate::error::ConversionError;
use crate::traits::AttributeValueConvert;

/// Represents a complete DynamoDB key specification with attribute names and values.
#[derive(Debug, Clone)]
pub struct KeySpec {
    partition_keys: Vec<(String, AttributeValue)>,
    sort_keys: Option<Vec<(String, AttributeValue)>>,
}

impl KeySpec {
    /// Creates a new KeySpec for a partition-key-only table.
    pub fn partition_only<PK: AttributeValueConvert>(
        pk_names: &[&str],
        pk_value: PK,
    ) -> Result<Self, ConversionError> {
        let av = pk_value.to_attribute_value()?;
        let partition_keys = Self::expand_key(pk_names, av)?;
        Ok(Self {
            partition_keys,
            sort_keys: None,
        })
    }

    /// Creates a new KeySpec for a composite-key table.
    pub fn composite<PK: AttributeValueConvert, SK: AttributeValueConvert>(
        pk_names: &[&str],
        pk_value: PK,
        sk_names: &[&str],
        sk_value: SK,
    ) -> Result<Self, ConversionError> {
        let pk_av = pk_value.to_attribute_value()?;
        let sk_av = sk_value.to_attribute_value()?;

        let partition_keys = Self::expand_key(pk_names, pk_av)?;
        let sort_keys = Some(Self::expand_key(sk_names, sk_av)?);

        Ok(Self {
            partition_keys,
            sort_keys,
        })
    }

    /// Creates a KeySpec from an object implementing ExtractKey.
    pub fn from_extract_key<T>(
        item: &T,
        pk_names: &[&str],
        sk_names: Option<&[&str]>,
    ) -> Result<Self, ConversionError>
    where
        T: crate::traits::ExtractKey,
    {
        let pk_av = item.extract_partition_key().to_attribute_value()?;
        let partition_keys = Self::expand_key(pk_names, pk_av)?;

        let sort_keys = match (sk_names, item.extract_sort_key()) {
            (Some(names), Some(sk)) => Some(Self::expand_key(names, sk.to_attribute_value()?)?),
            _ => None,
        };

        Ok(Self {
            partition_keys,
            sort_keys,
        })
    }

    /// Converts this KeySpec to a HashMap suitable for DynamoDB API calls.
    pub fn to_key_map(&self) -> HashMap<String, AttributeValue> {
        let mut map = HashMap::new();
        for (name, value) in &self.partition_keys {
            map.insert(name.clone(), value.clone());
        }
        if let Some(ref sk) = self.sort_keys {
            for (name, value) in sk {
                map.insert(name.clone(), value.clone());
            }
        }
        map
    }

    // Expands a single AttributeValue into multiple name/value pairs for multi-attribute keys
    fn expand_key(
        names: &[&str],
        value: AttributeValue,
    ) -> Result<Vec<(String, AttributeValue)>, ConversionError> {
        if names.len() == 1 {
            // Single-attribute key
            return Ok(vec![(names[0].to_string(), value)]);
        }

        // Multi-attribute key: value should be a list
        match value {
            AttributeValue::L(list) if list.len() == names.len() => Ok(names
                .iter()
                .zip(list)
                .map(|(name, av)| (name.to_string(), av))
                .collect()),
            AttributeValue::L(list) => Err(ConversionError::invalid_value(
                "",
                format!(
                    "key has {} attributes but value has {} elements",
                    names.len(),
                    list.len()
                ),
            )),
            // Single value for single-attribute key
            _ if names.len() == 1 => Ok(vec![(names[0].to_string(), value)]),
            _ => Err(ConversionError::invalid_value(
                "",
                format!(
                    "expected list for multi-attribute key with {} attributes",
                    names.len()
                ),
            )),
        }
    }
}

/// Helper trait for creating KeySpec from partition-key-only tables.
impl KeySpec {
    /// Creates a KeySpec for NoSortKey tables (partition-key-only).
    pub fn partition_only_no_sort<PK: AttributeValueConvert>(
        pk_names: &[&str],
        pk_value: PK,
    ) -> Result<Self, ConversionError> {
        Self::partition_only(pk_names, pk_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_partition_key() {
        let spec = KeySpec::partition_only(&["id"], "user123".to_string()).unwrap();
        let map = spec.to_key_map();
        assert_eq!(map.len(), 1);
        assert!(matches!(map.get("id"), Some(AttributeValue::S(s)) if s == "user123"));
    }

    #[test]
    fn test_composite_key() {
        let spec = KeySpec::composite(&["pk"], "user123".to_string(), &["sk"], 42i64).unwrap();
        let map = spec.to_key_map();
        assert_eq!(map.len(), 2);
        assert!(matches!(map.get("pk"), Some(AttributeValue::S(s)) if s == "user123"));
        assert!(matches!(map.get("sk"), Some(AttributeValue::N(n)) if n == "42"));
    }

    #[test]
    fn test_multi_attribute_partition_key() {
        let spec = KeySpec::partition_only(
            &["tournament_id", "region"],
            ("WINTER2024".to_string(), "NA-EAST".to_string()),
        )
        .unwrap();
        let map = spec.to_key_map();
        assert_eq!(map.len(), 2);
        assert!(
            matches!(map.get("tournament_id"), Some(AttributeValue::S(s)) if s == "WINTER2024")
        );
        assert!(matches!(map.get("region"), Some(AttributeValue::S(s)) if s == "NA-EAST"));
    }
}
