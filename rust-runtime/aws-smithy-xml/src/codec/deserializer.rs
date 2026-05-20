/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! XML deserializer implementing the `ShapeDeserializer` trait.

use super::XmlCodecSettings;
use crate::decode::{self, Document};
use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
use aws_smithy_schema::Schema;
use aws_smithy_types::date_time::Format as TimestampFormat;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document as SmithyDocument};
use std::borrow::Cow;
use std::sync::Arc;

/// XML deserializer that implements the `ShapeDeserializer` trait.
///
/// Wraps the existing `aws_smithy_xml::decode` SAX-like API and provides
/// schema-driven dispatch for struct members, lists, and maps.
pub struct XmlDeserializer<'a> {
    /// The parsing state — either we own the document (top-level) or we have
    /// pre-extracted text content (leaf scalar inside an element).
    state: DeserState<'a>,
    settings: Arc<XmlCodecSettings>,
}

enum DeserState<'a> {
    /// Top-level: owns the Document, ready to parse structure/elements.
    Doc(Document<'a>),
    /// Leaf: holds already-extracted text content for scalar reads.
    Text(Cow<'a, str>),
}

impl<'a> XmlDeserializer<'a> {
    /// Creates a new XML deserializer over raw bytes.
    pub(crate) fn new(input: &'a [u8], settings: Arc<XmlCodecSettings>) -> Self {
        let doc = Document::try_from(input).unwrap_or_else(|_| {
            // If UTF-8 conversion fails, create an empty document.
            // Actual errors will surface when trying to read elements.
            Document::new("")
        });
        Self {
            state: DeserState::Doc(doc),
            settings,
        }
    }

    /// Creates a deserializer positioned at already-extracted text content.
    fn from_text(text: Cow<'a, str>, settings: Arc<XmlCodecSettings>) -> Self {
        Self {
            state: DeserState::Text(text),
            settings,
        }
    }

    /// Extract the text content from the current state.
    fn take_text(&mut self) -> Result<Cow<'a, str>, SerdeError> {
        match &mut self.state {
            DeserState::Text(t) => Ok(std::mem::replace(t, Cow::Borrowed(""))),
            DeserState::Doc(doc) => {
                // Read text from the document's current position (inside an element scope).
                decode::try_data(doc).map_err(|e| SerdeError::custom(e.to_string()))
            }
        }
    }

    /// Resolve a child element name to a member schema by matching against
    /// @xmlName (if present) or member_name.
    fn resolve_member<'s>(schema: &'s Schema, element_name: &str) -> Option<&'s Schema> {
        schema.members().iter().copied().find(|m| {
            if let Some(xml_name) = m.xml_name() {
                xml_name.value() == element_name
            } else {
                m.member_name() == Some(element_name)
            }
        })
    }

    fn resolve_timestamp_format(&self, schema: &Schema) -> TimestampFormat {
        schema
            .timestamp_format()
            .map(|t| match t.format() {
                aws_smithy_schema::traits::TimestampFormat::EpochSeconds => {
                    TimestampFormat::EpochSeconds
                }
                aws_smithy_schema::traits::TimestampFormat::DateTime => TimestampFormat::DateTime,
                aws_smithy_schema::traits::TimestampFormat::HttpDate => TimestampFormat::HttpDate,
            })
            .unwrap_or(self.settings.default_timestamp_format())
    }
}

impl ShapeDeserializer for XmlDeserializer<'_> {
    fn read_struct(
        &mut self,
        schema: &Schema,
        consumer: &mut dyn FnMut(&Schema, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let doc = match &mut self.state {
            DeserState::Doc(doc) => doc,
            DeserState::Text(_) => {
                return Err(SerdeError::custom("expected XML element, found text"));
            }
        };

        let mut root = doc
            .root_element()
            .map_err(|e| SerdeError::custom(e.to_string()))?;

        // Dispatch @xmlAttribute members from the start element's attributes.
        for member in schema.members() {
            if member.xml_attribute() {
                let attr_name = member
                    .xml_name()
                    .map(|t| t.value())
                    .or(member.member_name())
                    .unwrap_or("");
                if let Some(value) = root.start_el().attr(attr_name) {
                    let mut child_deser = XmlDeserializer::from_text(
                        Cow::Owned(value.to_owned()),
                        self.settings.clone(),
                    );
                    consumer(member, &mut child_deser)?;
                }
            }
        }

        // Dispatch child elements.
        while let Some(mut child_scope) = root.next_tag() {
            let local = child_scope.start_el().local().to_owned();
            if let Some(member) = Self::resolve_member(schema, &local) {
                let text = decode::try_data(&mut child_scope)
                    .map_err(|e| SerdeError::custom(e.to_string()))?;
                let mut child_deser = XmlDeserializer::from_text(text, self.settings.clone());
                consumer(member, &mut child_deser)?;
            }
        }
        Ok(())
    }

    fn read_list(
        &mut self,
        _schema: &Schema,
        consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let doc = match &mut self.state {
            DeserState::Doc(doc) => doc,
            DeserState::Text(_) => {
                return Err(SerdeError::custom(
                    "expected XML element for list, found text",
                ));
            }
        };

        let mut root = doc
            .root_element()
            .map_err(|e| SerdeError::custom(e.to_string()))?;

        // Each child tag is a list item (e.g. <member>value</member>).
        while let Some(mut child_scope) = root.next_tag() {
            let text = decode::try_data(&mut child_scope)
                .map_err(|e| SerdeError::custom(e.to_string()))?;
            let mut child_deser = XmlDeserializer::from_text(text, self.settings.clone());
            consumer(&mut child_deser)?;
        }
        Ok(())
    }

    fn read_map(
        &mut self,
        schema: &Schema,
        consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let doc = match &mut self.state {
            DeserState::Doc(doc) => doc,
            DeserState::Text(_) => {
                return Err(SerdeError::custom(
                    "expected XML element for map, found text",
                ));
            }
        };

        let mut root = doc
            .root_element()
            .map_err(|e| SerdeError::custom(e.to_string()))?;

        // Resolve key/value element names from the schema.
        let key_name = schema
            .key()
            .and_then(|k| k.xml_name().map(|t| t.value()))
            .unwrap_or("key");
        let value_name = schema
            .member()
            .and_then(|v| v.xml_name().map(|t| t.value()))
            .unwrap_or("value");

        // Each child tag is an entry (e.g. <entry><key>k</key><value>v</value></entry>).
        while let Some(mut entry_scope) = root.next_tag() {
            let mut key = None;
            let mut value = None;
            while let Some(mut field_scope) = entry_scope.next_tag() {
                let local = field_scope.start_el().local().to_owned();
                let text = decode::try_data(&mut field_scope)
                    .map_err(|e| SerdeError::custom(e.to_string()))?;
                if local == key_name {
                    key = Some(text.into_owned());
                } else if local == value_name {
                    value = Some(text);
                }
            }
            if let (Some(k), Some(v)) = (key, value) {
                let mut child_deser = XmlDeserializer::from_text(v, self.settings.clone());
                consumer(k, &mut child_deser)?;
            }
        }
        Ok(())
    }

    fn read_boolean(&mut self, _schema: &Schema) -> Result<bool, SerdeError> {
        let text = self.take_text()?;
        match text.as_ref() {
            "true" => Ok(true),
            "false" => Ok(false),
            other => Err(SerdeError::custom(format!("invalid boolean: {other}"))),
        }
    }

    fn read_byte(&mut self, _schema: &Schema) -> Result<i8, SerdeError> {
        let text = self.take_text()?;
        text.parse().map_err(|e| SerdeError::custom(format!("{e}")))
    }

    fn read_short(&mut self, _schema: &Schema) -> Result<i16, SerdeError> {
        let text = self.take_text()?;
        text.parse().map_err(|e| SerdeError::custom(format!("{e}")))
    }

    fn read_integer(&mut self, _schema: &Schema) -> Result<i32, SerdeError> {
        let text = self.take_text()?;
        text.parse().map_err(|e| SerdeError::custom(format!("{e}")))
    }

    fn read_long(&mut self, _schema: &Schema) -> Result<i64, SerdeError> {
        let text = self.take_text()?;
        text.parse().map_err(|e| SerdeError::custom(format!("{e}")))
    }

    fn read_float(&mut self, _schema: &Schema) -> Result<f32, SerdeError> {
        let text = self.take_text()?;
        match text.as_ref() {
            "NaN" => Ok(f32::NAN),
            "Infinity" => Ok(f32::INFINITY),
            "-Infinity" => Ok(f32::NEG_INFINITY),
            _ => text.parse().map_err(|e| SerdeError::custom(format!("{e}"))),
        }
    }

    fn read_double(&mut self, _schema: &Schema) -> Result<f64, SerdeError> {
        let text = self.take_text()?;
        match text.as_ref() {
            "NaN" => Ok(f64::NAN),
            "Infinity" => Ok(f64::INFINITY),
            "-Infinity" => Ok(f64::NEG_INFINITY),
            _ => text.parse().map_err(|e| SerdeError::custom(format!("{e}"))),
        }
    }

    fn read_big_integer(&mut self, _schema: &Schema) -> Result<BigInteger, SerdeError> {
        let text = self.take_text()?;
        text.parse().map_err(|e| SerdeError::custom(format!("{e}")))
    }

    fn read_big_decimal(&mut self, _schema: &Schema) -> Result<BigDecimal, SerdeError> {
        let text = self.take_text()?;
        text.parse().map_err(|e| SerdeError::custom(format!("{e}")))
    }

    fn read_string(&mut self, _schema: &Schema) -> Result<String, SerdeError> {
        let text = self.take_text()?;
        Ok(text.into_owned())
    }

    fn read_blob(&mut self, _schema: &Schema) -> Result<Blob, SerdeError> {
        let text = self.take_text()?;
        let bytes = aws_smithy_types::base64::decode(text.as_ref())
            .map_err(|e| SerdeError::custom(format!("{e}")))?;
        Ok(Blob::new(bytes))
    }

    fn read_timestamp(&mut self, schema: &Schema) -> Result<DateTime, SerdeError> {
        let text = self.take_text()?;
        let format = self.resolve_timestamp_format(schema);
        DateTime::from_str(text.as_ref(), format).map_err(|e| SerdeError::custom(format!("{e}")))
    }

    fn read_document(&mut self, _schema: &Schema) -> Result<SmithyDocument, SerdeError> {
        Err(SerdeError::custom(
            "document types are not supported by REST XML",
        ))
    }

    fn is_null(&self) -> bool {
        // XML represents absence by omitting the element entirely.
        // If we have a deserializer, the element exists, so it's not null.
        false
    }

    fn container_size(&self) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_schema::{shape_id, Schema, ShapeType};

    static STRING_MEMBER: Schema =
        Schema::new_member(shape_id!("test", "S$v"), ShapeType::String, "v", 0);

    #[test]
    fn read_string_from_text_state() {
        let settings = Arc::new(XmlCodecSettings::default());
        let mut deser = XmlDeserializer::from_text(Cow::Borrowed("hello"), settings);
        let result = deser.read_string(&STRING_MEMBER).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn read_string_from_doc_text_content() {
        let xml = b"<root>world</root>";
        let settings = Arc::new(XmlCodecSettings::default());
        let mut deser = XmlDeserializer::new(xml, settings);
        // Advance past the root element start tag by entering root_element scope
        if let DeserState::Doc(ref mut doc) = deser.state {
            let mut scope = doc.root_element().unwrap();
            let text = decode::try_data(&mut scope).unwrap();
            assert_eq!(text, "world");
        }
    }

    #[test]
    fn is_null_always_false() {
        let settings = Arc::new(XmlCodecSettings::default());
        let deser = XmlDeserializer::new(b"<r/>", settings);
        assert!(!deser.is_null());
    }

    // --- Phase 4.2 tests ---

    static NAME_MEMBER: Schema = Schema::new_member(
        shape_id!("test", "Person$name"),
        ShapeType::String,
        "name",
        0,
    );
    static AGE_MEMBER: Schema =
        Schema::new_member(shape_id!("test", "Person$age"), ShapeType::String, "age", 1);
    static RENAMED_MEMBER: Schema = Schema::new_member(
        shape_id!("test", "Person$nick"),
        ShapeType::String,
        "nick",
        2,
    )
    .with_xml_name("Nickname");

    static PERSON_SCHEMA: Schema = Schema::new_struct(
        shape_id!("test", "Person"),
        ShapeType::Structure,
        &[&NAME_MEMBER, &AGE_MEMBER, &RENAMED_MEMBER],
    );

    #[test]
    fn read_struct_dispatches_members() {
        let xml = b"<Person><name>Alice</name><age>30</age></Person>";
        let settings = Arc::new(XmlCodecSettings::default());
        let mut deser = XmlDeserializer::new(xml, settings);

        let mut name = String::new();
        let mut age = String::new();
        deser
            .read_struct(&PERSON_SCHEMA, &mut |member, d| {
                match member.member_name().unwrap() {
                    "name" => name = d.read_string(member)?,
                    "age" => age = d.read_string(member)?,
                    _ => {}
                }
                Ok(())
            })
            .unwrap();

        assert_eq!(name, "Alice");
        assert_eq!(age, "30");
    }

    #[test]
    fn read_struct_skips_unknown_elements() {
        let xml = b"<Person><unknown>x</unknown><name>Bob</name></Person>";
        let settings = Arc::new(XmlCodecSettings::default());
        let mut deser = XmlDeserializer::new(xml, settings);

        let mut name = String::new();
        deser
            .read_struct(&PERSON_SCHEMA, &mut |member, d| {
                if member.member_name() == Some("name") {
                    name = d.read_string(member)?;
                }
                Ok(())
            })
            .unwrap();

        assert_eq!(name, "Bob");
    }

    #[test]
    fn read_struct_resolves_xml_name() {
        let xml = b"<Person><Nickname>Ally</Nickname></Person>";
        let settings = Arc::new(XmlCodecSettings::default());
        let mut deser = XmlDeserializer::new(xml, settings);

        let mut nick = String::new();
        deser
            .read_struct(&PERSON_SCHEMA, &mut |member, d| {
                if member.member_name() == Some("nick") {
                    nick = d.read_string(member)?;
                }
                Ok(())
            })
            .unwrap();

        assert_eq!(nick, "Ally");
    }

    // --- Phase 4.3 tests ---

    static ATTR_MEMBER: Schema =
        Schema::new_member(shape_id!("test", "X$id"), ShapeType::String, "id", 0)
            .with_xml_attribute();
    static ELEM_MEMBER: Schema =
        Schema::new_member(shape_id!("test", "X$name"), ShapeType::String, "name", 1);

    static X_SCHEMA: Schema = Schema::new_struct(
        shape_id!("test", "X"),
        ShapeType::Structure,
        &[&ATTR_MEMBER, &ELEM_MEMBER],
    );

    #[test]
    fn read_struct_dispatches_attributes() {
        let xml = b"<X id=\"42\"><name>hello</name></X>";
        let settings = Arc::new(XmlCodecSettings::default());
        let mut deser = XmlDeserializer::new(xml, settings);

        let mut id = String::new();
        let mut name = String::new();
        deser
            .read_struct(&X_SCHEMA, &mut |member, d| {
                match member.member_name().unwrap() {
                    "id" => id = d.read_string(member)?,
                    "name" => name = d.read_string(member)?,
                    _ => {}
                }
                Ok(())
            })
            .unwrap();

        assert_eq!(id, "42");
        assert_eq!(name, "hello");
    }

    // --- Phase 4.4 tests ---

    #[test]
    fn read_list_wrapped() {
        let xml = b"<items><member>a</member><member>b</member></items>";
        let settings = Arc::new(XmlCodecSettings::default());
        let mut deser = XmlDeserializer::new(xml, settings);

        static LIST_MEMBER: Schema = Schema::new_member(
            shape_id!("test", "L$member"),
            ShapeType::String,
            "member",
            0,
        );
        static LIST_SCHEMA: Schema = Schema::new_list(shape_id!("test", "L"), &LIST_MEMBER);

        let mut items = Vec::new();
        deser
            .read_list(&LIST_SCHEMA, &mut |d| {
                items.push(d.read_string(&LIST_MEMBER)?);
                Ok(())
            })
            .unwrap();

        assert_eq!(items, vec!["a", "b"]);
    }

    #[test]
    fn read_map_wrapped() {
        let xml = b"<myMap><entry><key>k1</key><value>v1</value></entry><entry><key>k2</key><value>v2</value></entry></myMap>";
        let settings = Arc::new(XmlCodecSettings::default());
        let mut deser = XmlDeserializer::new(xml, settings);

        static MAP_KEY: Schema =
            Schema::new_member(shape_id!("test", "M$key"), ShapeType::String, "key", 0);
        static MAP_VALUE: Schema =
            Schema::new_member(shape_id!("test", "M$value"), ShapeType::String, "value", 0);
        static MAP_SCHEMA: Schema = Schema::new_map(shape_id!("test", "M"), &MAP_KEY, &MAP_VALUE);

        let mut entries = Vec::new();
        deser
            .read_map(&MAP_SCHEMA, &mut |k, d| {
                entries.push((k, d.read_string(&MAP_VALUE)?));
                Ok(())
            })
            .unwrap();

        assert_eq!(
            entries,
            vec![
                ("k1".to_owned(), "v1".to_owned()),
                ("k2".to_owned(), "v2".to_owned())
            ]
        );
    }

    #[test]
    fn read_map_with_renamed_key_value() {
        let xml = b"<m><entry><Attribute>a</Attribute><Setting>s</Setting></entry></m>";
        let settings = Arc::new(XmlCodecSettings::default());
        let mut deser = XmlDeserializer::new(xml, settings);

        static MAP_KEY: Schema =
            Schema::new_member(shape_id!("test", "M$key"), ShapeType::String, "key", 0)
                .with_xml_name("Attribute");
        static MAP_VALUE: Schema =
            Schema::new_member(shape_id!("test", "M$value"), ShapeType::String, "value", 0)
                .with_xml_name("Setting");
        static MAP_SCHEMA: Schema = Schema::new_map(shape_id!("test", "M"), &MAP_KEY, &MAP_VALUE);

        let mut entries = Vec::new();
        deser
            .read_map(&MAP_SCHEMA, &mut |k, d| {
                entries.push((k, d.read_string(&MAP_VALUE)?));
                Ok(())
            })
            .unwrap();

        assert_eq!(entries, vec![("a".to_owned(), "s".to_owned())]);
    }

    // --- Phase 4.5 tests ---

    #[test]
    fn read_struct_flattened_list() {
        // Flattened list: repeated <item> siblings inside the struct.
        let xml = b"<S><name>hi</name><item>a</item><item>b</item></S>";
        let settings = Arc::new(XmlCodecSettings::default());
        let mut deser = XmlDeserializer::new(xml, settings);

        static S_NAME: Schema =
            Schema::new_member(shape_id!("test", "S$name"), ShapeType::String, "name", 0);
        static S_ITEMS: Schema =
            Schema::new_member(shape_id!("test", "S$items"), ShapeType::List, "items", 1)
                .with_xml_flattened()
                .with_xml_name("item");
        static S_SCHEMA: Schema = Schema::new_struct(
            shape_id!("test", "S"),
            ShapeType::Structure,
            &[&S_NAME, &S_ITEMS],
        );

        let mut name = String::new();
        let mut items = Vec::new();
        deser
            .read_struct(&S_SCHEMA, &mut |member, d| {
                match member.member_name().unwrap() {
                    "name" => name = d.read_string(member)?,
                    "items" => items.push(d.read_string(member)?),
                    _ => {}
                }
                Ok(())
            })
            .unwrap();

        assert_eq!(name, "hi");
        assert_eq!(items, vec!["a", "b"]);
    }

    #[test]
    fn read_struct_flattened_list_intermixed() {
        // Flattened list elements intermixed with other members.
        let xml = b"<S><item>x</item><name>n</name><item>y</item></S>";
        let settings = Arc::new(XmlCodecSettings::default());
        let mut deser = XmlDeserializer::new(xml, settings);

        static S_NAME: Schema =
            Schema::new_member(shape_id!("test", "S$name"), ShapeType::String, "name", 0);
        static S_ITEMS: Schema =
            Schema::new_member(shape_id!("test", "S$items"), ShapeType::List, "items", 1)
                .with_xml_flattened()
                .with_xml_name("item");
        static S_SCHEMA: Schema = Schema::new_struct(
            shape_id!("test", "S"),
            ShapeType::Structure,
            &[&S_NAME, &S_ITEMS],
        );

        let mut name = String::new();
        let mut items = Vec::new();
        deser
            .read_struct(&S_SCHEMA, &mut |member, d| {
                match member.member_name().unwrap() {
                    "name" => name = d.read_string(member)?,
                    "items" => items.push(d.read_string(member)?),
                    _ => {}
                }
                Ok(())
            })
            .unwrap();

        assert_eq!(name, "n");
        assert_eq!(items, vec!["x", "y"]);
    }

    // --- Phase 4.6 tests ---

    #[test]
    fn read_scalars() {
        let settings = Arc::new(XmlCodecSettings::default());

        let mut d = XmlDeserializer::from_text(Cow::Borrowed("true"), settings.clone());
        assert!(d.read_boolean(&STRING_MEMBER).unwrap());

        let mut d = XmlDeserializer::from_text(Cow::Borrowed("-42"), settings.clone());
        assert_eq!(d.read_integer(&STRING_MEMBER).unwrap(), -42);

        let mut d = XmlDeserializer::from_text(Cow::Borrowed("NaN"), settings.clone());
        assert!(d.read_float(&STRING_MEMBER).unwrap().is_nan());

        let mut d = XmlDeserializer::from_text(Cow::Borrowed("Infinity"), settings.clone());
        assert_eq!(d.read_double(&STRING_MEMBER).unwrap(), f64::INFINITY);

        let mut d = XmlDeserializer::from_text(Cow::Borrowed("aGVsbG8="), settings.clone());
        assert_eq!(d.read_blob(&STRING_MEMBER).unwrap().as_ref(), b"hello");

        let mut d =
            XmlDeserializer::from_text(Cow::Borrowed("2023-04-01T12:00:00Z"), settings.clone());
        let ts = d.read_timestamp(&STRING_MEMBER).unwrap();
        assert_eq!(ts.secs(), 1680350400);
    }

    #[test]
    fn read_document_returns_error() {
        let settings = Arc::new(XmlCodecSettings::default());
        let mut deser = XmlDeserializer::from_text(Cow::Borrowed("x"), settings);
        assert_eq!(
            deser.read_document(&STRING_MEMBER).unwrap_err().to_string(),
            "document types are not supported by REST XML"
        );
    }
}
