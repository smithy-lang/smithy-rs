/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use serde::{Serialize, Serializer, Deserialize, Deserializer};
use serde_json::Value;
use smithy_types::{Document, Number};
use serde::de::{Visitor, Error, SeqAccess, MapAccess};
use std::fmt::Formatter;
use std::fmt;
use std::collections::HashMap;

#[allow(unused)]
pub fn json_to_doc(json: Value) -> Document {
    match json {
        Value::Null => Document::Null,
        Value::Bool(b) => Document::Bool(b),
        Value::Number(num) => Document::Number(serde_num_to_num(&num)),
        Value::String(str) => Document::String(str),
        Value::Array(arr) => Document::Array(arr.into_iter().map(json_to_doc).collect()),
        Value::Object(map) => {
            Document::Object(map.into_iter().map(|(k, v)| (k, json_to_doc(v))).collect())
        }
    }
}

pub struct SerDoc<'a>(pub &'a Document);
pub struct DeserDoc(pub Document);

impl Serialize for SerDoc<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error>
    where
        S: Serializer,
    {
        let doc = &self.0;
        Ok(match doc {
            Document::Object(obj) => {
                serializer.collect_map(obj.iter().map(|(k, v)| (k, SerDoc(v))))?
            }
            Document::Array(arr) => serializer.collect_seq(arr.iter().map(|doc| SerDoc(doc)))?,
            Document::Number(Number::PosInt(n)) => serializer.serialize_u64(*n)?,
            Document::Number(Number::NegInt(n)) => serializer.serialize_i64(*n)?,
            Document::Number(Number::Float(n)) => serializer.serialize_f64(*n)?,
            Document::String(string) => serializer.serialize_str(&string)?,
            Document::Bool(bool) => serializer.serialize_bool(*bool)?,
            Document::Null => serializer.serialize_none()?,
        })
    }
}

struct DocVisitor;
impl<'de> Visitor<'de> for DocVisitor {
    type Value = Document;

    fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "Expecting a JSON-like document")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> where
        E: Error, {
        Ok(Document::Bool(v))
    }

    fn visit_i8<E>(self, v: i8) -> Result<Self::Value, E> where
        E: Error, {
        Ok(Document::Number(serde_num_to_num(&serde_json::Number::from(v))))
    }

    fn visit_i16<E>(self, v: i16) -> Result<Self::Value, E> where
        E: Error, {
        Ok(Document::Number(serde_num_to_num(&serde_json::Number::from(v))))
    }

    fn visit_i32<E>(self, v: i32) -> Result<Self::Value, E> where
        E: Error, {
        Ok(Document::Number(serde_num_to_num(&serde_json::Number::from(v))))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> where
        E: Error, {
        Ok(Document::Number(serde_num_to_num(&serde_json::Number::from(v))))
    }

    fn visit_u8<E>(self, v: u8) -> Result<Self::Value, E> where
        E: Error, {
        Ok(Document::Number(serde_num_to_num(&serde_json::Number::from(v))))
    }

    fn visit_u16<E>(self, v: u16) -> Result<Self::Value, E> where
        E: Error, {
        Ok(Document::Number(serde_num_to_num(&serde_json::Number::from(v))))
    }

    fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E> where
        E: Error, {
        Ok(Document::Number(serde_num_to_num(&serde_json::Number::from(v))))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> where
        E: Error, {
        Ok(Document::Number(serde_num_to_num(&serde_json::Number::from(v))))
    }

    fn visit_f32<E>(self, v: f32) -> Result<Self::Value, E> where
        E: Error, {
        Ok(Document::Number(Number::Float(v as _)))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> where
        E: Error, {
        Ok(Document::Number(Number::Float(v as _)))
    }

    fn visit_char<E>(self, v: char) -> Result<Self::Value, E> where
        E: Error, {
        Ok(Document::String(v.to_string()))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> where
        E: Error, {
        Ok(Document::String(v.to_string()))

    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, <A as SeqAccess<'de>>::Error> where
        A: SeqAccess<'de>, {
        let mut out: Vec<Document> = vec![];
        while let Some(next) = seq.next_element::<DeserDoc>()? {
            out.push(next.0);
        }
        Ok(Document::Array(out))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, <A as MapAccess<'de>>::Error> where
        A: MapAccess<'de>, {
        let mut out: HashMap<String, Document> = HashMap::new();
        while let Some((k, v)) = map.next_entry::<String, DeserDoc>()? {
            out.insert(k, v.0);
        }
        Ok(Document::Object(out))
    }
}

impl<'de> Deserialize<'de> for DeserDoc {
    fn deserialize<D>(deserializer: D) -> Result<Self, <D as Deserializer<'de>>::Error>
        where
            D: Deserializer<'de>
    {
        Ok(DeserDoc(deserializer.deserialize_any(DocVisitor)?))
    }
}

fn serde_num_to_num(number: &serde_json::Number) -> smithy_types::Number {
    if number.is_f64() {
        smithy_types::Number::Float(number.as_f64().unwrap())
    } else if number.is_i64() {
        smithy_types::Number::NegInt(number.as_i64().unwrap())
    } else if number.is_u64() {
        smithy_types::Number::PosInt(number.as_u64().unwrap())
    } else {
        panic!("Serde nums should be either f64, i64 or u64")
    }
}
