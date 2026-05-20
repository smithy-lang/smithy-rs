/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! XML serializer.
//!
//! Implements the schema-serde [`ShapeSerializer`] trait for the AWS REST XML
//! protocol. Serialization is single-pass with a deferred start-tag flush:
//! when a struct's start tag is written, the closing `>` is held back until
//! either an attribute is added (Phase 3.3) or the first child element /
//! text is written. This lets us emit attributes inline with the start tag
//! without buffering the start tag separately.
//!
//! Phase 3 sub-tasks:
//!  - 3.1 (this commit): frame state machine, `write_struct`, `write_string`.
//!  - 3.2: scalar member writes.
//!  - 3.3: attribute writes via `xml_attribute()`.
//!  - 3.4: namespace emission on root element.
//!  - 3.5: list serialization.
//!  - 3.6: map serialization.
//!  - 3.7: `write_document` returns `SerdeError`.
//!  - 3.8: codec wiring.

use super::XmlCodecSettings;
use aws_smithy_schema::codec::FinishSerializer;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::Schema;
use aws_smithy_types::date_time::Format as TimestampFormat;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};
use std::sync::Arc;

/// XML serializer that implements the [`ShapeSerializer`] trait.
pub struct XmlSerializer {
    output: String,
    settings: Arc<XmlCodecSettings>,
    /// Stack of open elements. Top of stack is the deepest currently-open
    /// element. The frame state distinguishes "start tag still open" from
    /// "start tag closed, inside body".
    frames: Vec<Frame>,
}

#[derive(Debug)]
enum Frame {
    /// `<name` has been written; the closing `>` is deferred so that
    /// attributes (Phase 3.3) and namespaces (Phase 3.4) can still be added
    /// inline with the start tag. The element name is held so we can emit
    /// the matching `</name>` close tag later, or self-close as `/>` if no
    /// children are written.
    StartTagPending { name: String },
    /// `<name>` (or `<name attr="...">`) has been fully written; we are now
    /// inside the element body. Children may follow.
    Open { name: String },
}

impl XmlSerializer {
    /// Creates a new XML serializer with the given settings.
    pub(crate) fn new(settings: Arc<XmlCodecSettings>) -> Self {
        Self {
            output: String::new(),
            settings,
            frames: Vec::new(),
        }
    }

    /// Resolve the XML element name for a schema being serialized as an element.
    ///
    /// Resolution order:
    /// 1. `@xmlName` (member-level wins over shape-level via the codegen-emitted
    ///    member schema).
    /// 2. `original_name` — the synthetic shape's pre-rename name. Set only on
    ///    operation input/output synthetic shapes.
    /// 3. `member_name` — the smithy member name, set on member schemas.
    /// 4. `shape_id().shape_name()` — fallback for non-synthetic, non-member shapes.
    fn element_name(schema: &Schema) -> &str {
        schema
            .xml_name()
            .map(|t| t.value())
            .or_else(|| schema.original_name())
            .or_else(|| schema.member_name())
            .unwrap_or_else(|| schema.shape_id().shape_name())
    }

    /// If the top frame is a [`Frame::StartTagPending`], close its start tag
    /// (write `>`) and transition the frame to [`Frame::Open`]. No-op
    /// otherwise. Called before any child content (text or nested element)
    /// is written.
    fn flush_start_tag(&mut self) {
        if let Some(frame) = self.frames.last_mut() {
            if let Frame::StartTagPending { name } = frame {
                self.output.push('>');
                let name = std::mem::take(name);
                *frame = Frame::Open { name };
            }
        }
    }

    /// Write `<name` and push a new [`Frame::StartTagPending`]. The caller
    /// must have already flushed any parent's pending start tag (via
    /// [`Self::flush_start_tag`]) so the new element doesn't end up nested
    /// inside an unclosed tag.
    fn open_element(&mut self, name: &str) {
        self.output.push('<');
        self.output.push_str(name);
        self.frames.push(Frame::StartTagPending {
            name: name.to_owned(),
        });
    }

    /// Pop the top frame and emit the closing tag. If the frame is still in
    /// [`Frame::StartTagPending`] (no children were written), emits `/>` to
    /// self-close; otherwise emits `</name>`.
    fn close_element(&mut self) {
        let frame = self
            .frames
            .pop()
            .expect("close_element called with empty frame stack");
        match frame {
            Frame::StartTagPending { .. } => self.output.push_str("/>"),
            Frame::Open { name } => {
                self.output.push_str("</");
                self.output.push_str(&name);
                self.output.push('>');
            }
        }
    }

    /// Emit `<name>content</name>` where `content` is already safe (no
    /// XML-special chars). Used for numbers, booleans, base64 — values that
    /// are known not to need escaping.
    fn write_safe_element(&mut self, schema: &Schema, content: &str) {
        use std::fmt::Write;
        self.flush_start_tag();
        let name = Self::element_name(schema);
        write!(self.output, "<{name}>{content}</{name}>").unwrap();
    }

    /// Resolve the timestamp format for a member. Member-level
    /// `@timestampFormat` wins; otherwise the codec's default (`date-time`
    /// for REST XML body bindings).
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

impl FinishSerializer for XmlSerializer {
    fn finish(self) -> Vec<u8> {
        debug_assert!(
            self.frames.is_empty(),
            "XmlSerializer::finish called with {} unclosed frame(s)",
            self.frames.len()
        );
        self.output.into_bytes()
    }
}

impl ShapeSerializer for XmlSerializer {
    fn write_struct(
        &mut self,
        schema: &Schema,
        value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        // Close any parent's open start tag before opening our own element.
        // No-op at the document root.
        self.flush_start_tag();
        let name = Self::element_name(schema);
        self.open_element(name);
        // Phase 3.4 will inject namespace declarations here while the frame
        // is still in StartTagPending.
        value.serialize_members(self)?;
        self.close_element();
        Ok(())
    }

    fn write_list(
        &mut self,
        _schema: &Schema,
        _write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_list — implemented in Phase 3.5")
    }

    fn write_map(
        &mut self,
        _schema: &Schema,
        _write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_map — implemented in Phase 3.6")
    }

    fn write_boolean(&mut self, schema: &Schema, value: bool) -> Result<(), SerdeError> {
        self.write_safe_element(schema, if value { "true" } else { "false" });
        Ok(())
    }

    fn write_byte(&mut self, schema: &Schema, value: i8) -> Result<(), SerdeError> {
        use std::fmt::Write;
        self.flush_start_tag();
        let name = Self::element_name(schema);
        write!(self.output, "<{name}>{value}</{name}>").unwrap();
        Ok(())
    }

    fn write_short(&mut self, schema: &Schema, value: i16) -> Result<(), SerdeError> {
        use std::fmt::Write;
        self.flush_start_tag();
        let name = Self::element_name(schema);
        write!(self.output, "<{name}>{value}</{name}>").unwrap();
        Ok(())
    }

    fn write_integer(&mut self, schema: &Schema, value: i32) -> Result<(), SerdeError> {
        use std::fmt::Write;
        self.flush_start_tag();
        let name = Self::element_name(schema);
        write!(self.output, "<{name}>{value}</{name}>").unwrap();
        Ok(())
    }

    fn write_long(&mut self, schema: &Schema, value: i64) -> Result<(), SerdeError> {
        use std::fmt::Write;
        self.flush_start_tag();
        let name = Self::element_name(schema);
        write!(self.output, "<{name}>{value}</{name}>").unwrap();
        Ok(())
    }

    fn write_float(&mut self, schema: &Schema, value: f32) -> Result<(), SerdeError> {
        let text = if value.is_nan() {
            "NaN".to_owned()
        } else if value.is_infinite() {
            if value.is_sign_positive() {
                "Infinity".to_owned()
            } else {
                "-Infinity".to_owned()
            }
        } else {
            value.to_string()
        };
        self.write_safe_element(schema, &text);
        Ok(())
    }

    fn write_double(&mut self, schema: &Schema, value: f64) -> Result<(), SerdeError> {
        let text = if value.is_nan() {
            "NaN".to_owned()
        } else if value.is_infinite() {
            if value.is_sign_positive() {
                "Infinity".to_owned()
            } else {
                "-Infinity".to_owned()
            }
        } else {
            value.to_string()
        };
        self.write_safe_element(schema, &text);
        Ok(())
    }

    fn write_big_integer(&mut self, schema: &Schema, value: &BigInteger) -> Result<(), SerdeError> {
        self.write_safe_element(schema, value.as_ref());
        Ok(())
    }

    fn write_big_decimal(&mut self, schema: &Schema, value: &BigDecimal) -> Result<(), SerdeError> {
        self.write_safe_element(schema, value.as_ref());
        Ok(())
    }

    fn write_string(&mut self, schema: &Schema, value: &str) -> Result<(), SerdeError> {
        use std::fmt::Write;
        self.flush_start_tag();
        let name = Self::element_name(schema);
        let escaped = crate::escape::escape(value);
        write!(self.output, "<{name}>{escaped}</{name}>").unwrap();
        Ok(())
    }

    fn write_blob(&mut self, schema: &Schema, value: &Blob) -> Result<(), SerdeError> {
        let encoded = aws_smithy_types::base64::encode(value.as_ref());
        self.write_safe_element(schema, &encoded);
        Ok(())
    }

    fn write_timestamp(&mut self, schema: &Schema, value: &DateTime) -> Result<(), SerdeError> {
        let format = self.resolve_timestamp_format(schema);
        let formatted = value
            .fmt(format)
            .map_err(|e| SerdeError::custom(e.to_string()))?;
        // Timestamp text is safe (digits, dashes, colons, T, Z, dots) — no escaping needed.
        self.write_safe_element(schema, &formatted);
        Ok(())
    }

    fn write_document(&mut self, _schema: &Schema, _value: &Document) -> Result<(), SerdeError> {
        todo!("XmlSerializer::write_document — returns SerdeError in Phase 3.7 (REST XML does not support documents)")
    }

    fn write_null(&mut self, _schema: &Schema) -> Result<(), SerdeError> {
        // XML represents null/absent members by omitting the element entirely.
        // Generated code skips None fields, so this should rarely be called.
        // If it is (e.g. sparse collections), we simply emit nothing.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_schema::{shape_id, Schema, ShapeType};

    /// Renders a struct with one string member named `name`.
    static NAME_MEMBER: Schema = Schema::new_member(
        shape_id!("test", "Person$name"),
        ShapeType::String,
        "name",
        0,
    );
    static PERSON_SCHEMA: Schema = Schema::new_struct(
        shape_id!("test", "Person"),
        ShapeType::Structure,
        &[&NAME_MEMBER],
    );

    struct Person<'a> {
        name: &'a str,
    }

    impl SerializableStruct for Person<'_> {
        fn serialize_members(
            &self,
            serializer: &mut dyn ShapeSerializer,
        ) -> Result<(), SerdeError> {
            serializer.write_string(&NAME_MEMBER, self.name)
        }
    }

    fn serialize<F>(write: F) -> String
    where
        F: FnOnce(&mut XmlSerializer) -> Result<(), SerdeError>,
    {
        let mut ser = XmlSerializer::new(Arc::new(XmlCodecSettings::default()));
        write(&mut ser).expect("serialization failed");
        String::from_utf8(<XmlSerializer as FinishSerializer>::finish(ser)).unwrap()
    }

    #[test]
    fn struct_with_string_member() {
        let p = Person { name: "Iago" };
        let out = serialize(|ser| ser.write_struct(&PERSON_SCHEMA, &p));
        assert_eq!(out, "<Person><name>Iago</name></Person>");
    }

    #[test]
    fn struct_with_no_members_self_closes() {
        struct Empty;
        impl SerializableStruct for Empty {
            fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                Ok(())
            }
        }
        static EMPTY_SCHEMA: Schema =
            Schema::new_struct(shape_id!("test", "Empty"), ShapeType::Structure, &[]);

        let out = serialize(|ser| ser.write_struct(&EMPTY_SCHEMA, &Empty));
        assert_eq!(out, "<Empty/>");
    }

    #[test]
    fn struct_string_value_is_escaped() {
        let p = Person { name: "<a&b>" };
        let out = serialize(|ser| ser.write_struct(&PERSON_SCHEMA, &p));
        assert_eq!(out, "<Person><name>&lt;a&amp;b&gt;</name></Person>");
    }

    #[test]
    fn struct_string_value_eol_is_encoded() {
        // Per the XML EOL Encoding SEP, \r and \n must be escaped as
        // numeric character references to survive XML EOL normalization.
        let p = Person { name: "a\r\nb" };
        let out = serialize(|ser| ser.write_struct(&PERSON_SCHEMA, &p));
        assert_eq!(out, "<Person><name>a&#xD;&#xA;b</name></Person>");
    }

    #[test]
    fn nested_structs_close_correctly() {
        // Schemas: Outer { inner: Inner { name: String } }.
        // Note: the inner-struct schema is not exercised directly because
        // member dispatch in this codec uses the *member* schema, not the
        // target shape's schema. The Inner type's `SerializableStruct` impl
        // is what drives inner serialization.
        static INNER_NAME: Schema = Schema::new_member(
            shape_id!("test", "Inner$name"),
            ShapeType::String,
            "name",
            0,
        );
        static OUTER_INNER: Schema = Schema::new_member(
            shape_id!("test", "Outer$inner"),
            ShapeType::Structure,
            "inner",
            0,
        );
        static OUTER_SCHEMA: Schema = Schema::new_struct(
            shape_id!("test", "Outer"),
            ShapeType::Structure,
            &[&OUTER_INNER],
        );

        struct Inner<'a> {
            name: &'a str,
        }
        impl SerializableStruct for Inner<'_> {
            fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                ser.write_string(&INNER_NAME, self.name)
            }
        }
        struct Outer<'a> {
            inner: Inner<'a>,
        }
        impl SerializableStruct for Outer<'_> {
            fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                // For nested structs, dispatch on the *member* schema so that
                // the resolved element name is the field's name (or its
                // @xmlName), not the target shape's name.
                ser.write_struct(&OUTER_INNER, &self.inner)
            }
        }

        let o = Outer {
            inner: Inner { name: "v" },
        };
        let out = serialize(|ser| ser.write_struct(&OUTER_SCHEMA, &o));
        assert_eq!(out, "<Outer><inner><name>v</name></inner></Outer>");
    }

    #[test]
    fn xml_name_overrides_member_name() {
        static RENAMED_MEMBER: Schema = Schema::new_member(
            shape_id!("test", "Person$name"),
            ShapeType::String,
            "name",
            0,
        )
        .with_xml_name("FullName");
        static PERSON_SCHEMA: Schema = Schema::new_struct(
            shape_id!("test", "Person"),
            ShapeType::Structure,
            &[&RENAMED_MEMBER],
        );

        struct P;
        impl SerializableStruct for P {
            fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                ser.write_string(&RENAMED_MEMBER, "v")
            }
        }

        let out = serialize(|ser| ser.write_struct(&PERSON_SCHEMA, &P));
        assert_eq!(out, "<Person><FullName>v</FullName></Person>");
    }

    #[test]
    fn original_name_overrides_id_for_synthetic_root() {
        // Synthetic shapes have id name "OperationInput" but original_name is
        // "OperationRequest" (the user-authored name). The codec should use
        // the original name for the root element when there's no @xmlName.
        static SYNTHETIC: Schema = Schema::new_struct(
            shape_id!("test.synthetic", "FooInput"),
            ShapeType::Structure,
            &[],
        )
        .with_original_name("FooRequest");

        struct Empty;
        impl SerializableStruct for Empty {
            fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                Ok(())
            }
        }

        let out = serialize(|ser| ser.write_struct(&SYNTHETIC, &Empty));
        assert_eq!(out, "<FooRequest/>");
    }

    // --- Phase 3.2 scalar tests ---

    static SCALAR_MEMBER: Schema =
        Schema::new_member(shape_id!("test", "S$v"), ShapeType::Integer, "v", 0);

    #[test]
    fn write_boolean_true() {
        let out = serialize(|ser| ser.write_boolean(&SCALAR_MEMBER, true));
        assert_eq!(out, "<v>true</v>");
    }

    #[test]
    fn write_boolean_false() {
        let out = serialize(|ser| ser.write_boolean(&SCALAR_MEMBER, false));
        assert_eq!(out, "<v>false</v>");
    }

    #[test]
    fn write_integer_negative() {
        let out = serialize(|ser| ser.write_integer(&SCALAR_MEMBER, -42));
        assert_eq!(out, "<v>-42</v>");
    }

    #[test]
    fn write_long_large() {
        let out = serialize(|ser| ser.write_long(&SCALAR_MEMBER, i64::MAX));
        assert_eq!(out, format!("<v>{}</v>", i64::MAX));
    }

    #[test]
    fn write_float_special_values() {
        let out = serialize(|ser| ser.write_float(&SCALAR_MEMBER, f32::NAN));
        assert_eq!(out, "<v>NaN</v>");
        let out = serialize(|ser| ser.write_float(&SCALAR_MEMBER, f32::INFINITY));
        assert_eq!(out, "<v>Infinity</v>");
        let out = serialize(|ser| ser.write_float(&SCALAR_MEMBER, f32::NEG_INFINITY));
        assert_eq!(out, "<v>-Infinity</v>");
    }

    #[test]
    fn write_double_normal() {
        let out = serialize(|ser| ser.write_double(&SCALAR_MEMBER, 1.5));
        assert_eq!(out, "<v>1.5</v>");
    }

    #[test]
    fn write_blob_base64() {
        let blob = Blob::new(b"hello");
        let out = serialize(|ser| ser.write_blob(&SCALAR_MEMBER, &blob));
        assert_eq!(out, "<v>aGVsbG8=</v>");
    }

    #[test]
    fn write_timestamp_default_datetime() {
        // Default format for REST XML is date-time (ISO 8601).
        let ts = DateTime::from_secs(1515531081);
        let out = serialize(|ser| ser.write_timestamp(&SCALAR_MEMBER, &ts));
        assert_eq!(out, "<v>2018-01-09T20:51:21Z</v>");
    }

    #[test]
    fn write_timestamp_epoch_seconds_override() {
        use aws_smithy_schema::traits::TimestampFormat as SchemaTimestampFormat;
        static TS_MEMBER: Schema =
            Schema::new_member(shape_id!("test", "S$t"), ShapeType::Timestamp, "t", 0)
                .with_timestamp_format(SchemaTimestampFormat::EpochSeconds);
        let ts = DateTime::from_secs(1515531081);
        let out = serialize(|ser| ser.write_timestamp(&TS_MEMBER, &ts));
        assert_eq!(out, "<t>1515531081</t>");
    }

    #[test]
    fn write_null_emits_nothing() {
        let out = serialize(|ser| ser.write_null(&SCALAR_MEMBER));
        assert_eq!(out, "");
    }
}
