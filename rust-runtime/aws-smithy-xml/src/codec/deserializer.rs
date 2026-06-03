/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::borrow::Cow;

use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
use aws_smithy_schema::Schema;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};

/// Default maximum nesting depth for XML deserialization.
pub const DEFAULT_MAX_DEPTH: u32 = 128;

pub struct XmlDeserializerSettings {
    max_depth: u32,
}

impl Default for XmlDeserializerSettings {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
        }
    }
}

impl XmlDeserializerSettings {
    pub fn max_depth(&self) -> u32 {
        self.max_depth
    }
}

pub struct XmlShapeDeserializer<'a> {
    input: &'a str,
    depth: u32,
    max_depth: u32,
}

impl<'a> XmlShapeDeserializer<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self::with_settings(input, &XmlDeserializerSettings::default())
    }

    pub fn with_settings(input: &'a [u8], settings: &XmlDeserializerSettings) -> Self {
        Self {
            input: std::str::from_utf8(input).unwrap_or(""),
            depth: 0,
            max_depth: settings.max_depth,
        }
    }

    /// Creates a deserializer from a string slice with explicit depth tracking.
    /// Use `depth=1` when the envelope has already been stripped (skips root element stripping).
    pub fn from_str(input: &'a str, depth: u32, max_depth: u32) -> Self {
        Self {
            input,
            depth,
            max_depth,
        }
    }
}

fn resolve_xml_member<'a>(schema: &'a Schema, element_name: &str) -> Option<&'a Schema> {
    schema
        .members()
        .iter()
        .find(
            |m| match m.xml_name().map(|t| t.value()).or(m.member_name()) {
                Some(wire_name) => wire_name == element_name,
                None => false,
            },
        )
        .copied()
}

struct XmlElement {
    name: String,
    content_start: usize,
    content_end: usize,
}

fn parse_top_level_children(xml: &str) -> Vec<XmlElement> {
    use xmlparser::{ElementEnd, Token, Tokenizer};
    let mut wrapped = String::with_capacity(xml.len() + 7);
    wrapped.push_str("<_>");
    wrapped.push_str(xml);
    wrapped.push_str("</_>");
    let mut results = Vec::new();
    let mut tokenizer = Tokenizer::from(wrapped.as_str());
    let mut depth: u32 = 0;
    let mut current_name: Option<String> = None;
    let mut content_start: usize = 0;
    let prefix_len: usize = 3; // "<_>"

    while let Some(Ok(token)) = tokenizer.next() {
        match token {
            Token::ElementStart { local, .. } => {
                depth += 1;
                if depth == 2 {
                    current_name = Some(local.as_str().to_string());
                }
            }
            Token::ElementEnd { end, span } => match end {
                ElementEnd::Open => {
                    if depth == 2 {
                        content_start = span.end() - prefix_len;
                    }
                }
                ElementEnd::Close(_, _) => {
                    if depth == 2 {
                        let content_end = span.start() - prefix_len;
                        if let Some(name) = current_name.take() {
                            results.push(XmlElement {
                                name,
                                content_start,
                                content_end,
                            });
                        }
                    }
                    depth -= 1;
                }
                ElementEnd::Empty => {
                    if depth == 2 {
                        if let Some(name) = current_name.take() {
                            results.push(XmlElement {
                                name,
                                content_start: 0,
                                content_end: 0,
                            });
                        }
                    }
                    depth -= 1;
                }
            },
            _ => {}
        }
    }
    results
}

fn strip_root_element(xml: &str) -> &str {
    use xmlparser::{ElementEnd, Token, Tokenizer};
    let mut tokenizer = Tokenizer::from(xml);
    let mut depth: u32 = 0;
    let mut root_content_start: usize = 0;
    let mut root_content_end: usize = xml.len();
    while let Some(Ok(token)) = tokenizer.next() {
        match token {
            Token::ElementStart { .. } => {
                depth += 1;
            }
            Token::ElementEnd { end, span } => match end {
                ElementEnd::Open => {
                    if depth == 1 {
                        root_content_start = span.end();
                    }
                }
                ElementEnd::Close(_, _) => {
                    if depth == 1 {
                        root_content_end = span.start();
                        break;
                    }
                    depth -= 1;
                }
                ElementEnd::Empty => {
                    if depth == 1 {
                        return "";
                    }
                    depth -= 1;
                }
            },
            _ => {}
        }
    }
    &xml[root_content_start..root_content_end]
}

impl<'a> ShapeDeserializer for XmlShapeDeserializer<'a> {
    fn read_struct(
        &mut self,
        schema: &Schema,
        consumer: &mut dyn FnMut(&Schema, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err(SerdeError::custom("maximum nesting depth exceeded"));
        }
        if self.input.is_empty() {
            self.depth -= 1;
            return Ok(());
        }
        let inner = if self.depth == 1 {
            strip_root_element(self.input)
        } else {
            self.input
        };
        let children = parse_top_level_children(inner);
        let mut consumed: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for child in &children {
            if consumed.contains(child.name.as_str()) {
                continue;
            }
            if let Some(member_schema) = resolve_xml_member(schema, &child.name) {
                if member_schema.xml_flattened().is_some() {
                    consumed.insert(&child.name);
                    let mut collected = String::new();
                    for c in children.iter().filter(|c| c.name == child.name) {
                        let content = if c.content_start == 0 && c.content_end == 0 {
                            ""
                        } else {
                            &inner[c.content_start..c.content_end]
                        };
                        collected.push_str("<i>");
                        collected.push_str(content);
                        collected.push_str("</i>");
                    }
                    let mut sub =
                        XmlShapeDeserializer::from_str(&collected, self.depth, self.max_depth);
                    consumer(member_schema, &mut sub)?;
                } else {
                    let content = if child.content_start == 0 && child.content_end == 0 {
                        ""
                    } else {
                        &inner[child.content_start..child.content_end]
                    };
                    let mut sub =
                        XmlShapeDeserializer::from_str(content, self.depth, self.max_depth);
                    consumer(member_schema, &mut sub)?;
                }
            }
        }
        self.depth -= 1;
        Ok(())
    }

    fn read_list(
        &mut self,
        _schema: &Schema,
        consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err(SerdeError::custom("maximum nesting depth exceeded"));
        }
        if self.input.is_empty() {
            self.depth -= 1;
            return Ok(());
        }
        let inner = if self.depth == 1 {
            strip_root_element(self.input)
        } else {
            self.input
        };
        let children = parse_top_level_children(inner);
        for child in &children {
            let content = if child.content_start == 0 && child.content_end == 0 {
                ""
            } else {
                &inner[child.content_start..child.content_end]
            };
            let mut sub = XmlShapeDeserializer::from_str(content, self.depth, self.max_depth);
            consumer(&mut sub)?;
        }
        self.depth -= 1;
        Ok(())
    }

    fn read_map(
        &mut self,
        schema: &Schema,
        consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.depth += 1;
        if self.depth > self.max_depth {
            return Err(SerdeError::custom("maximum nesting depth exceeded"));
        }
        if self.input.is_empty() {
            self.depth -= 1;
            return Ok(());
        }
        let inner = if self.depth == 1 {
            strip_root_element(self.input)
        } else {
            self.input
        };
        let key_element = schema.xml_map_key_name().unwrap_or("key");
        let value_element = schema.xml_map_value_name().unwrap_or("value");
        let entries = parse_top_level_children(inner);
        for entry in &entries {
            if entry.content_start == 0 && entry.content_end == 0 {
                continue;
            }
            let entry_content = &inner[entry.content_start..entry.content_end];
            let kv_children = parse_top_level_children(entry_content);
            let mut key = String::new();
            let mut value_content = String::new();
            for kv in &kv_children {
                let c = if kv.content_start == 0 && kv.content_end == 0 {
                    ""
                } else {
                    &entry_content[kv.content_start..kv.content_end]
                };
                if kv.name == key_element {
                    key = unescape_xml(c).into_owned();
                } else if kv.name == value_element {
                    value_content = c.to_string();
                }
            }
            let mut sub =
                XmlShapeDeserializer::from_str(&value_content, self.depth, self.max_depth);
            consumer(key, &mut sub)?;
        }
        self.depth -= 1;
        Ok(())
    }

    fn read_boolean(&mut self, _schema: &Schema) -> Result<bool, SerdeError> {
        match self.input.trim() {
            "true" => Ok(true),
            "false" => Ok(false),
            other => Err(SerdeError::TypeMismatch {
                message: format!("expected boolean, got: {other}"),
            }),
        }
    }

    fn read_byte(&mut self, _schema: &Schema) -> Result<i8, SerdeError> {
        self.input
            .trim()
            .parse()
            .map_err(|e| SerdeError::InvalidInput {
                message: format!("{e}"),
            })
    }

    fn read_short(&mut self, _schema: &Schema) -> Result<i16, SerdeError> {
        self.input
            .trim()
            .parse()
            .map_err(|e| SerdeError::InvalidInput {
                message: format!("{e}"),
            })
    }

    fn read_integer(&mut self, _schema: &Schema) -> Result<i32, SerdeError> {
        self.input
            .trim()
            .parse()
            .map_err(|e| SerdeError::InvalidInput {
                message: format!("{e}"),
            })
    }

    fn read_long(&mut self, _schema: &Schema) -> Result<i64, SerdeError> {
        self.input
            .trim()
            .parse()
            .map_err(|e| SerdeError::InvalidInput {
                message: format!("{e}"),
            })
    }

    fn read_float(&mut self, _schema: &Schema) -> Result<f32, SerdeError> {
        let s = self.input.trim();
        match s {
            "NaN" => Ok(f32::NAN),
            "Infinity" => Ok(f32::INFINITY),
            "-Infinity" => Ok(f32::NEG_INFINITY),
            _ => s.parse().map_err(|e| SerdeError::InvalidInput {
                message: format!("{e}"),
            }),
        }
    }

    fn read_double(&mut self, _schema: &Schema) -> Result<f64, SerdeError> {
        let s = self.input.trim();
        match s {
            "NaN" => Ok(f64::NAN),
            "Infinity" => Ok(f64::INFINITY),
            "-Infinity" => Ok(f64::NEG_INFINITY),
            _ => s.parse().map_err(|e| SerdeError::InvalidInput {
                message: format!("{e}"),
            }),
        }
    }

    fn read_big_integer(&mut self, _schema: &Schema) -> Result<BigInteger, SerdeError> {
        use std::str::FromStr;
        BigInteger::from_str(self.input.trim()).map_err(|e| SerdeError::InvalidInput {
            message: format!("{e}"),
        })
    }

    fn read_big_decimal(&mut self, _schema: &Schema) -> Result<BigDecimal, SerdeError> {
        use std::str::FromStr;
        BigDecimal::from_str(self.input.trim()).map_err(|e| SerdeError::InvalidInput {
            message: format!("{e}"),
        })
    }

    fn read_string(&mut self, _schema: &Schema) -> Result<String, SerdeError> {
        Ok(unescape_xml(self.input).into_owned())
    }

    fn read_blob(&mut self, _schema: &Schema) -> Result<Blob, SerdeError> {
        let decoded = aws_smithy_types::base64::decode(self.input.trim()).map_err(|e| {
            SerdeError::InvalidInput {
                message: format!("invalid base64: {e}"),
            }
        })?;
        Ok(Blob::new(decoded))
    }

    fn read_timestamp(&mut self, schema: &Schema) -> Result<DateTime, SerdeError> {
        let s = self.input.trim();
        let format = if let Some(ts_trait) = schema.timestamp_format() {
            match ts_trait.format() {
                aws_smithy_schema::traits::TimestampFormat::EpochSeconds => {
                    aws_smithy_types::date_time::Format::EpochSeconds
                }
                aws_smithy_schema::traits::TimestampFormat::HttpDate => {
                    aws_smithy_types::date_time::Format::HttpDate
                }
                aws_smithy_schema::traits::TimestampFormat::DateTime => {
                    aws_smithy_types::date_time::Format::DateTimeWithOffset
                }
            }
        } else {
            aws_smithy_types::date_time::Format::DateTimeWithOffset
        };
        DateTime::from_str(s, format)
            .map_err(|e| SerdeError::custom(format!("invalid timestamp: {e}")))
    }

    fn read_document(&mut self, _schema: &Schema) -> Result<Document, SerdeError> {
        Err(SerdeError::UnsupportedOperation {
            message: "documents not supported in XML".into(),
        })
    }

    fn is_null(&self) -> bool {
        false
    }

    fn read_null(&mut self) -> Result<(), SerdeError> {
        Ok(())
    }

    fn container_size(&self) -> Option<usize> {
        None
    }
}

fn unescape_xml(s: &str) -> Cow<'_, str> {
    // Delegate to the crate's single-pass unescape which also handles numeric refs.
    crate::unescape::unescape(s).unwrap_or(Cow::Borrowed(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_schema::{shape_id, ShapeType};

    static NAME_MEMBER: Schema =
        Schema::new_member(shape_id!("test", "S"), ShapeType::String, "Name", 0);
    static AGE_MEMBER: Schema =
        Schema::new_member(shape_id!("test", "S"), ShapeType::Integer, "Age", 1);
    static SCHEMA: Schema = Schema::new_struct(
        shape_id!("test", "S"),
        ShapeType::Structure,
        &[&NAME_MEMBER, &AGE_MEMBER],
    );

    #[test]
    fn read_simple_struct() {
        let xml = b"<Result><Name>Alice</Name><Age>30</Age></Result>";
        let mut deser = XmlShapeDeserializer::new(xml);
        let mut name = String::new();
        let mut age = 0i32;
        deser
            .read_struct(&SCHEMA, &mut |member, d| {
                match member.member_name() {
                    Some("Name") => name = d.read_string(member)?,
                    Some("Age") => age = d.read_integer(member)?,
                    _ => {}
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(name, "Alice");
        assert_eq!(age, 30);
    }

    #[test]
    fn read_nested_struct() {
        static STREET: Schema =
            Schema::new_member(shape_id!("t", "Addr"), ShapeType::String, "Street", 0);
        static CITY: Schema =
            Schema::new_member(shape_id!("t", "Addr"), ShapeType::String, "City", 1);
        static ADDR_SCHEMA: Schema = Schema::new_struct(
            shape_id!("t", "Addr"),
            ShapeType::Structure,
            &[&STREET, &CITY],
        );
        static ADDR_MEMBER: Schema =
            Schema::new_member(shape_id!("t", "S"), ShapeType::Structure, "Address", 0);
        static OUTER: Schema =
            Schema::new_struct(shape_id!("t", "S"), ShapeType::Structure, &[&ADDR_MEMBER]);

        let xml =
            b"<Result><Address><Street>123 Main</Street><City>Seattle</City></Address></Result>";
        let mut deser = XmlShapeDeserializer::new(xml);
        let mut street = String::new();
        let mut city = String::new();
        deser
            .read_struct(&OUTER, &mut |member, d| {
                if member.member_name() == Some("Address") {
                    d.read_struct(&ADDR_SCHEMA, &mut |inner_member, d2| {
                        match inner_member.member_name() {
                            Some("Street") => street = d2.read_string(inner_member)?,
                            Some("City") => city = d2.read_string(inner_member)?,
                            _ => {}
                        }
                        Ok(())
                    })?;
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(street, "123 Main");
        assert_eq!(city, "Seattle");
    }

    #[test]
    fn nested_struct_three_levels_deep() {
        static LEAF: Schema =
            Schema::new_member(shape_id!("t", "Inner"), ShapeType::String, "Leaf", 0);
        static INNER_SCHEMA: Schema =
            Schema::new_struct(shape_id!("t", "Inner"), ShapeType::Structure, &[&LEAF]);
        static INNER_MEMBER: Schema =
            Schema::new_member(shape_id!("t", "Middle"), ShapeType::Structure, "Inner", 0);
        static MIDDLE_SCHEMA: Schema = Schema::new_struct(
            shape_id!("t", "Middle"),
            ShapeType::Structure,
            &[&INNER_MEMBER],
        );
        static MIDDLE_MEMBER: Schema =
            Schema::new_member(shape_id!("t", "Outer"), ShapeType::Structure, "Middle", 0);
        static OUTER_SCHEMA: Schema = Schema::new_struct(
            shape_id!("t", "Outer"),
            ShapeType::Structure,
            &[&MIDDLE_MEMBER],
        );
        static OUTER_MEMBER: Schema =
            Schema::new_member(shape_id!("t", "Root"), ShapeType::Structure, "Outer", 0);
        static ROOT: Schema = Schema::new_struct(
            shape_id!("t", "Root"),
            ShapeType::Structure,
            &[&OUTER_MEMBER],
        );

        let xml = b"<Root><Outer><Middle><Inner><Leaf>value</Leaf></Inner></Middle></Outer></Root>";
        let mut deser = XmlShapeDeserializer::new(xml);
        let mut leaf = String::new();
        deser
            .read_struct(&ROOT, &mut |outer_m, d_outer| {
                assert_eq!(outer_m.member_name(), Some("Outer"));
                d_outer.read_struct(&OUTER_SCHEMA, &mut |middle_m, d_middle| {
                    assert_eq!(middle_m.member_name(), Some("Middle"));
                    d_middle.read_struct(&MIDDLE_SCHEMA, &mut |inner_m, d_inner| {
                        assert_eq!(inner_m.member_name(), Some("Inner"));
                        d_inner.read_struct(&INNER_SCHEMA, &mut |leaf_m, d_leaf| {
                            if leaf_m.member_name() == Some("Leaf") {
                                leaf = d_leaf.read_string(leaf_m)?;
                            }
                            Ok(())
                        })
                    })
                })
            })
            .expect("3-level nested struct should round-trip");
        assert_eq!(leaf, "value");
    }

    #[test]
    fn struct_member_with_escaped_text_round_trips() {
        static VALUE: Schema =
            Schema::new_member(shape_id!("t", "Body"), ShapeType::String, "value", 0);
        static BODY_SCHEMA: Schema =
            Schema::new_struct(shape_id!("t", "Body"), ShapeType::Structure, &[&VALUE]);
        static PAYLOAD_MEMBER: Schema = Schema::new_member(
            shape_id!("t", "Envelope"),
            ShapeType::Structure,
            "payload",
            0,
        );
        static ENVELOPE_SCHEMA: Schema = Schema::new_struct(
            shape_id!("t", "Envelope"),
            ShapeType::Structure,
            &[&PAYLOAD_MEMBER],
        );

        let xml = b"<Envelope><payload><value>foo&amp;bar</value></payload></Envelope>";
        let mut deser = XmlShapeDeserializer::new(xml);
        let mut value_str = String::new();
        deser
            .read_struct(&ENVELOPE_SCHEMA, &mut |payload_m, d_payload| {
                let _ = payload_m;
                d_payload.read_struct(&BODY_SCHEMA, &mut |inner_m, d_inner| {
                    if inner_m.member_name() == Some("value") {
                        value_str = d_inner.read_string(inner_m)?;
                    }
                    Ok(())
                })
            })
            .expect("escaped text inside a nested struct should round-trip");
        assert_eq!(value_str, "foo&bar");
    }

    #[test]
    fn unescape_does_not_double_unescape() {
        // &amp;lt; should become &lt; (not <)
        let xml = b"<S><Name>&amp;lt;b&amp;gt;</Name></S>";
        static NAME: Schema = Schema::new_member(shape_id!("t", "S"), ShapeType::String, "Name", 0);
        static S: Schema = Schema::new_struct(shape_id!("t", "S"), ShapeType::Structure, &[&NAME]);
        let mut deser = XmlShapeDeserializer::new(xml);
        let mut val = String::new();
        deser
            .read_struct(&S, &mut |m, d| {
                val = d.read_string(m)?;
                Ok(())
            })
            .unwrap();
        assert_eq!(val, "&lt;b&gt;");
    }

    #[test]
    fn read_list() {
        let xml = b"<Items><member>foo</member><member>bar</member></Items>";
        let mut deser = XmlShapeDeserializer::new(xml);
        let mut items = Vec::new();
        deser
            .read_list(&aws_smithy_schema::prelude::STRING, &mut |d| {
                items.push(d.read_string(&aws_smithy_schema::prelude::STRING)?);
                Ok(())
            })
            .unwrap();
        assert_eq!(items, vec!["foo", "bar"]);
    }

    #[test]
    fn read_map() {
        let xml = b"<Tags><entry><key>color</key><value>red</value></entry><entry><key>size</key><value>large</value></entry></Tags>";
        let mut deser = XmlShapeDeserializer::new(xml);
        let mut map = std::collections::HashMap::new();
        deser
            .read_map(&aws_smithy_schema::prelude::STRING, &mut |key, d| {
                map.insert(key, d.read_string(&aws_smithy_schema::prelude::STRING)?);
                Ok(())
            })
            .unwrap();
        assert_eq!(map.get("color").unwrap(), "red");
        assert_eq!(map.get("size").unwrap(), "large");
    }

    #[test]
    fn read_map_with_empty_key() {
        let xml = b"<Tags><entry><key></key><value>empty</value></entry></Tags>";
        let mut deser = XmlShapeDeserializer::new(xml);
        let mut map = std::collections::HashMap::new();
        deser
            .read_map(&aws_smithy_schema::prelude::STRING, &mut |key, d| {
                map.insert(key, d.read_string(&aws_smithy_schema::prelude::STRING)?);
                Ok(())
            })
            .unwrap();
        assert_eq!(map.get("").unwrap(), "empty");
    }

    #[test]
    fn read_boolean() {
        let mut deser = XmlShapeDeserializer::new(b"true");
        assert!(deser
            .read_boolean(&aws_smithy_schema::prelude::BOOLEAN)
            .unwrap());
    }

    #[test]
    fn read_float_special_values() {
        let mut deser = XmlShapeDeserializer::new(b"NaN");
        assert!(deser
            .read_float(&aws_smithy_schema::prelude::FLOAT)
            .unwrap()
            .is_nan());

        let mut deser = XmlShapeDeserializer::new(b"Infinity");
        assert_eq!(
            deser
                .read_float(&aws_smithy_schema::prelude::FLOAT)
                .unwrap(),
            f32::INFINITY
        );
    }

    #[test]
    fn empty_input_as_empty_struct() {
        let mut deser = XmlShapeDeserializer::new(b"");
        deser.read_struct(&SCHEMA, &mut |_, _| Ok(())).unwrap();
    }

    #[test]
    fn unknown_elements_skipped() {
        let xml = b"<Result><Unknown>skip</Unknown><Name>Bob</Name></Result>";
        let mut deser = XmlShapeDeserializer::new(xml);
        let mut name = String::new();
        deser
            .read_struct(&SCHEMA, &mut |member, d| {
                if member.member_name() == Some("Name") {
                    name = d.read_string(member)?;
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(name, "Bob");
    }

    #[test]
    fn xml_name_trait() {
        static MEMBER: Schema =
            Schema::new_member(shape_id!("t", "S"), ShapeType::String, "myField", 0)
                .with_xml_name("CustomName");
        static S: Schema =
            Schema::new_struct(shape_id!("t", "S"), ShapeType::Structure, &[&MEMBER]);

        let xml = b"<Result><CustomName>hello</CustomName></Result>";
        let mut deser = XmlShapeDeserializer::new(xml);
        let mut val = String::new();
        deser
            .read_struct(&S, &mut |member, d| {
                val = d.read_string(member)?;
                Ok(())
            })
            .unwrap();
        assert_eq!(val, "hello");
    }

    #[test]
    fn is_null_returns_false_for_empty_element() {
        let deser = XmlShapeDeserializer::new(b"");
        assert!(!deser.is_null());

        let deser = XmlShapeDeserializer::new(b"some content");
        assert!(!deser.is_null());
    }

    #[test]
    fn empty_element_deserializes_as_empty_string() {
        let xml = b"<Result><Name></Name></Result>";
        let mut deser = XmlShapeDeserializer::new(xml);
        let mut name = String::from("unset");
        deser
            .read_struct(&SCHEMA, &mut |member, d| {
                if member.member_name() == Some("Name") {
                    name = d.read_string(member)?;
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(name, "");
    }

    #[test]
    fn map_with_struct_values_from_envelope() {
        static HI: Schema = Schema::new_member(shape_id!("t", "G"), ShapeType::String, "hi", 0);
        static GREETING: Schema =
            Schema::new_struct(shape_id!("t", "G"), ShapeType::Structure, &[&HI]);
        static MY_MAP: Schema = Schema::new_member(shape_id!("t", "O"), ShapeType::Map, "myMap", 0);
        static OUTPUT: Schema =
            Schema::new_struct(shape_id!("t", "O"), ShapeType::Structure, &[&MY_MAP]);

        let xml = b"<R><myMap><entry><key>foo</key><value><hi>there</hi></value></entry><entry><key>baz</key><value><hi>bye</hi></value></entry></myMap></R>";
        let mut deser = XmlShapeDeserializer::new(xml);
        let mut map = std::collections::HashMap::new();
        deser
            .read_struct(&OUTPUT, &mut |_member, d| {
                d.read_map(&GREETING, &mut |key, d2| {
                    d2.read_struct(&GREETING, &mut |m, d3| {
                        if m.member_name() == Some("hi") {
                            map.insert(key.clone(), d3.read_string(m)?);
                        }
                        Ok(())
                    })
                })
            })
            .unwrap();
        assert_eq!(map.get("foo").unwrap(), "there");
        assert_eq!(map.get("baz").unwrap(), "bye");
    }

    #[test]
    fn max_depth_exceeded() {
        let xml = b"<A><B>x</B></A>";
        let mut deser = XmlShapeDeserializer::from_str(
            std::str::from_utf8(xml).unwrap(),
            DEFAULT_MAX_DEPTH,
            DEFAULT_MAX_DEPTH,
        );
        static B: Schema = Schema::new_member(shape_id!("t", "S"), ShapeType::String, "B", 0);
        static S: Schema = Schema::new_struct(shape_id!("t", "S"), ShapeType::Structure, &[&B]);
        let result = deser.read_struct(&S, &mut |_, _| Ok(()));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("maximum nesting depth"));
    }

    #[test]
    fn flattened_list_collects_siblings() {
        static ITEM: Schema = Schema::new_member(shape_id!("t", "S"), ShapeType::List, "item", 0)
            .with_xml_flattened();
        static S: Schema = Schema::new_struct(shape_id!("t", "S"), ShapeType::Structure, &[&ITEM]);

        let xml = b"<R><item>hi</item><item>bye</item></R>";
        let mut deser = XmlShapeDeserializer::new(xml);
        let mut items = Vec::new();
        deser
            .read_struct(&S, &mut |_member, d| {
                d.read_list(&aws_smithy_schema::prelude::STRING, &mut |d2| {
                    items.push(d2.read_string(&aws_smithy_schema::prelude::STRING)?);
                    Ok(())
                })
            })
            .unwrap();
        assert_eq!(items, vec!["hi", "bye"]);
    }
}
