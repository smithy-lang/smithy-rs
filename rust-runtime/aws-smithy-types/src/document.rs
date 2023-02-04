/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::Number;
#[cfg(all(aws_sdk_unstable, feature = "deserialize"))]
use serde::Deserialize;
#[cfg(all(aws_sdk_unstable, feature = "serialize"))]
use serde::Serialize;
use std::collections::HashMap;

/* ANCHOR: document */

/// Document Type
///
/// Document types represents protocol-agnostic open content that is accessed like JSON data.
/// Open content is useful for modeling unstructured data that has no schema, data that can't be
/// modeled using rigid types, or data that has a schema that evolves outside of the purview of a model.
/// The serialization format of a document is an implementation detail of a protocol.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(all(aws_sdk_unstable, feature = "serialize"), derive(Serialize))]
#[cfg_attr(all(aws_sdk_unstable, feature = "deserialize"), derive(Deserialize))]
#[cfg_attr(
    any(
        all(aws_sdk_unstable, feature = "deserialize"),
        all(aws_sdk_unstable, feature = "serialize")
    ),
    serde(untagged)
)]
pub enum Document {
    /// JSON object
    Object(HashMap<String, Document>),
    /// JSON array
    Array(Vec<Document>),
    /// JSON number
    Number(Number),
    /// JSON string
    String(String),
    /// JSON boolean
    Bool(bool),
    /// JSON null
    Null,
}

impl From<bool> for Document {
    fn from(value: bool) -> Self {
        Document::Bool(value)
    }
}

impl From<String> for Document {
    fn from(value: String) -> Self {
        Document::String(value)
    }
}

impl From<Vec<Document>> for Document {
    fn from(values: Vec<Document>) -> Self {
        Document::Array(values)
    }
}

impl From<HashMap<String, Document>> for Document {
    fn from(values: HashMap<String, Document>) -> Self {
        Document::Object(values)
    }
}

impl From<u64> for Document {
    fn from(value: u64) -> Self {
        Document::Number(Number::PosInt(value))
    }
}

impl From<i64> for Document {
    fn from(value: i64) -> Self {
        Document::Number(Number::NegInt(value))
    }
}

impl From<i32> for Document {
    fn from(value: i32) -> Self {
        Document::Number(Number::NegInt(value as i64))
    }
}

/* ANCHOR END: document */

#[cfg(test)]
mod test {
    #[cfg(all(aws_sdk_unstable, feature = "serialize", feature = "deserialize"))]
    use crate::Document;
    #[cfg(all(aws_sdk_unstable, feature = "serialize", feature = "deserialize"))]
    use crate::Number;
    #[cfg(all(aws_sdk_unstable, feature = "serialize", feature = "deserialize"))]
    use std::collections::HashMap;

    /// checks if a) serialization of json suceeds and b) it is compatible with serde_json
    #[test]
    #[cfg(all(aws_sdk_unstable, feature = "serialize", feature = "deserialize"))]
    fn serialize_json() {
        use crate::Document;
        use crate::Number;
        use std::collections::HashMap;
        let mut map: HashMap<String, Document> = HashMap::new();
        // string
        map.insert("hello".into(), "world".to_string().into());
        // numbers
        map.insert("pos_int".into(), Document::Number(Number::PosInt(1).into()));
        map.insert(
            "neg_int".into(),
            Document::Number(Number::NegInt(-1).into()),
        );
        map.insert(
            "float".into(),
            Document::Number(Number::Float(0.1 + 0.2).into()),
        );
        // booleans
        map.insert("true".into(), true.into());
        map.insert("false".into(), false.into());
        // check if array with different datatypes would succeed
        map.insert(
            "array".into(),
            vec![
                map.clone().into(),
                "hello-world".to_string().into(),
                true.into(),
                false.into(),
            ]
            .into(),
        );
        // map
        map.insert("map".into(), map.clone().into());
        // null
        map.insert("null".into(), Document::Null);
        let obj = Document::Object(map);
        // comparing string isnt going to work since there is no gurantee for the ordering of the keys
        let target_file = include_str!("../test_data/serialize_document.json");
        let json: Result<serde_json::Value, _> = serde_json::from_str(target_file);
        // serializer
        assert_eq!(serde_json::to_value(&obj).unwrap(), json.unwrap());
        let doc: Result<Document, _> = serde_json::from_str(target_file);
        assert_eq!(obj, doc.unwrap());
    }
}
