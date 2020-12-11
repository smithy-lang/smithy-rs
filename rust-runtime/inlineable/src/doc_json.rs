use serde::{Serialize, Serializer};
use serde_json::Value;
use smithy_types::{Document, Number};

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
