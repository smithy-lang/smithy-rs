use serde::{Serialize, Serializer};
use serde_json::{Map, Value};
use smithy_types::{Document, Number};

#[allow(unused)]
pub enum SerializationError {
    // NaN/Infinity is not valid JSON
    InvalidF64,
}

#[allow(unused)]
pub fn doc_to_json(doc: &Document) -> Result<Value, SerializationError> {
    Ok(match doc {
        Document::Object(obj) => {
            let serde_map: Result<Map<String, Value>, SerializationError> = obj
                .iter()
                .map(|(k, v)| doc_to_json(v).map(|value| (k.clone(), value)))
                .collect();
            Value::Object(serde_map?)
        }
        Document::Array(docs) => {
            let result: Result<Vec<Value>, SerializationError> =
                docs.iter().map(doc_to_json).collect();
            Value::Array(result?)
        }
        Document::Number(number) => Value::Number(num_to_serde_num(number)?),
        Document::String(string) => Value::String(string.to_string()),
        Document::Bool(b) => Value::Bool(*b),
        Document::Null => Value::Null,
    })
}

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

fn num_to_serde_num(
    number: &smithy_types::Number,
) -> Result<serde_json::Number, SerializationError> {
    match number {
        smithy_types::Number::Float(num) => {
            serde_json::Number::from_f64(*num).ok_or(SerializationError::InvalidF64)
        }
        smithy_types::Number::NegInt(num) => Ok(serde_json::Number::from(*num)),
        smithy_types::Number::PosInt(num) => Ok(serde_json::Number::from(*num)),
    }
}
