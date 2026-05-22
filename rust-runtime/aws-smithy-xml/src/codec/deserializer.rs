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
///
/// The deserializer holds the input as `&'a [u8]` throughout. For aggregate
/// reads (`read_struct`, `read_list`, `read_map`) we construct a fresh
/// `Document` over `input` on demand. For scalar reads we either parse
/// `input` to extract the root element's text, or — when a parent
/// aggregate has already extracted the leaf text from its child element —
/// take the pre-extracted text directly via `text`.
///
/// Sibling dispatch within an aggregate read avoids per-iteration
/// `XmlDeserializer` construction by reusing `&mut self` through
/// `dispatch_subslice` / `dispatch_text` helpers, which save and restore
/// the relevant state across the closure call.
pub struct XmlDeserializer<'a> {
    /// XML bytes for this deserializer. For the document root and for
    /// aggregate sub-deserializers this is the slice of the parent input
    /// covering the relevant element. Ignored when `text` is set.
    input: &'a [u8],
    /// Pre-extracted leaf text. When `Some`, scalar reads consume it
    /// directly without re-parsing `input`; aggregate reads error.
    text: Option<Cow<'a, str>>,
    settings: Arc<XmlCodecSettings>,
    /// Optional schema override consulted by the next aggregate read
    /// (`read_struct` / `read_list` / `read_map`) when codegen passes a
    /// shapeless placeholder schema (e.g. `prelude::DOCUMENT`) for an
    /// inner aggregate. Used to thread the outer aggregate's
    /// `value_schema` / `member_schema` (with its own
    /// `with_map_members`/`with_list_member` chain) into nested reads so
    /// nested element-name overrides (`@xmlName` on inner key/value) can
    /// be honored.
    schema_override: Option<&'static Schema>,
}

impl<'a> XmlDeserializer<'a> {
    /// Creates a new XML deserializer over raw bytes.
    pub(crate) fn new(input: &'a [u8], settings: Arc<XmlCodecSettings>) -> Self {
        Self {
            input,
            text: None,
            settings,
            schema_override: None,
        }
    }

    /// Creates a deserializer pre-loaded with leaf text content. Used by
    /// tests; runtime dispatch uses [`dispatch_text`](Self::dispatch_text)
    /// to repoint an existing deserializer at leaf text rather than
    /// constructing a new instance.
    #[cfg(test)]
    fn from_text(text: Cow<'a, str>, settings: Arc<XmlCodecSettings>) -> Self {
        Self {
            input: b"",
            text: Some(text),
            settings,
            schema_override: None,
        }
    }

    /// Construct a fresh `Document` over `self.input`. Errors if the
    /// deserializer holds pre-extracted text (an aggregate read was
    /// expected on this deserializer).
    fn document(&self) -> Result<Document<'a>, SerdeError> {
        if self.text.is_some() {
            return Err(SerdeError::custom("expected XML element, found text"));
        }
        Ok(Document::try_from(self.input).unwrap_or_else(|_| Document::new("")))
    }

    /// Extract the leaf text content. If `text` was pre-set, returns it
    /// directly; otherwise parses `input`, navigates to the root element,
    /// and reads its text content.
    fn take_text(&mut self) -> Result<Cow<'a, str>, SerdeError> {
        if let Some(t) = self.text.take() {
            return Ok(t);
        }
        let mut doc = Document::try_from(self.input).unwrap_or_else(|_| Document::new(""));
        let mut root = doc
            .root_element()
            .map_err(|e| SerdeError::custom(e.to_string()))?;
        decode::try_data(&mut root).map_err(|e| SerdeError::custom(e.to_string()))
    }

    /// Run `f` against `self` after temporarily repointing it at a sub-slice
    /// of the parent input. State (input, text, schema_override) is
    /// saved on entry and restored on return so the deserializer can be
    /// reused for sibling dispatches without per-iteration allocation.
    fn dispatch_subslice<R>(
        &mut self,
        sub: &'a [u8],
        schema_override: Option<&'static Schema>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let saved_input = std::mem::replace(&mut self.input, sub);
        let saved_text = self.text.take();
        let saved_override = std::mem::replace(&mut self.schema_override, schema_override);
        let r = f(self);
        self.input = saved_input;
        self.text = saved_text;
        self.schema_override = saved_override;
        r
    }

    /// Run `f` against `self` after temporarily setting pre-extracted leaf
    /// text. State is restored on return.
    fn dispatch_text<R>(&mut self, text: Cow<'a, str>, f: impl FnOnce(&mut Self) -> R) -> R {
        let saved_input = std::mem::replace(&mut self.input, b"");
        let saved_text = self.text.replace(text);
        let saved_override = self.schema_override.take();
        let r = f(self);
        self.input = saved_input;
        self.text = saved_text;
        self.schema_override = saved_override;
        r
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

    /// Find the byte slice in `input` that contains the element whose local name
    /// pointer `el_local` points into `input`. Uses pointer arithmetic to locate
    /// the `<` before the element name, then scans forward for the matching close
    /// tag with depth tracking. Returns the sub-slice `<tag...>...</tag>`.
    fn find_element_slice(input: &'a [u8], el_local: &str) -> &'a [u8] {
        let input_str = std::str::from_utf8(input).unwrap_or("");
        let input_start = input_str.as_ptr() as usize;
        let name_ptr = el_local.as_ptr() as usize;

        // The element name is inside the input string. Find the `<` before it.
        // Account for possible prefix: `<prefix:local` — scan back past `:` and prefix.
        let name_offset = name_ptr.saturating_sub(input_start);
        let el_start = input_str[..name_offset].rfind('<').unwrap_or(0);

        // Find matching close tag by scanning with depth tracking.
        let tag_name = el_local;
        let remaining = &input_str[el_start..];
        let mut depth = 0i32;
        let mut pos = 0;
        while pos < remaining.len() {
            if remaining[pos..].starts_with("</") {
                // Close tag — check if it matches our tag name
                let after_slash = pos + 2;
                if remaining[after_slash..].starts_with(tag_name) {
                    let after_name = after_slash + tag_name.len();
                    if remaining.as_bytes().get(after_name) == Some(&b'>') {
                        depth -= 1;
                        if depth == 0 {
                            let end = el_start + after_name + 1;
                            return &input[el_start..end];
                        }
                    }
                }
                pos = after_slash;
            } else if remaining.as_bytes()[pos] == b'<'
                && remaining.as_bytes().get(pos + 1) != Some(&b'/')
                && remaining.as_bytes().get(pos + 1) != Some(&b'?')
            {
                // Open tag — check if self-closing or matches our name
                if let Some(gt) = remaining[pos..].find('>') {
                    let tag_content = &remaining[pos + 1..pos + gt];
                    let is_self_closing = tag_content.ends_with('/');
                    let opens_our_tag = tag_content.starts_with(tag_name)
                        && tag_content
                            .as_bytes()
                            .get(tag_name.len())
                            .map_or(true, |&b| b == b' ' || b == b'>' || b == b'/');
                    if opens_our_tag && !is_self_closing {
                        depth += 1;
                    }
                    pos += gt + 1;
                } else {
                    pos += 1;
                }
            } else {
                pos += 1;
            }
        }
        // Fallback: return from el_start to end
        &input[el_start..]
    }

    fn resolve_timestamp_format(&self, schema: &Schema) -> TimestampFormat {
        schema
            .timestamp_format()
            .map(|t| match t.format() {
                aws_smithy_schema::traits::TimestampFormat::EpochSeconds => {
                    TimestampFormat::EpochSeconds
                }
                // Use the lenient `DateTimeWithOffset` so timezone-suffixed
                // RFC-3339 strings (e.g. `2019-12-17T00:48:18+01:00`) parse —
                // matches the Smithy `date-time` protocol-test expectations
                // and the JSON codec's behavior.
                aws_smithy_schema::traits::TimestampFormat::DateTime => {
                    TimestampFormat::DateTimeWithOffset
                }
                aws_smithy_schema::traits::TimestampFormat::HttpDate => TimestampFormat::HttpDate,
            })
            .unwrap_or_else(|| match self.settings.default_timestamp_format() {
                TimestampFormat::DateTime => TimestampFormat::DateTimeWithOffset,
                other => other,
            })
    }
}

impl ShapeDeserializer for XmlDeserializer<'_> {
    fn read_struct(
        &mut self,
        schema: &Schema,
        consumer: &mut dyn FnMut(&Schema, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        // Empty input → empty struct. Mirrors how the JSON codec treats an
        // empty body: the consumer is never called, the builder takes its
        // defaults, and the builder's @httpHeader / @httpResponseCode reads
        // (which happen *outside* read_struct, in `deserialize_with_response`)
        // are still free to populate the rest of the output. This is what
        // S3 HEAD operations and any other header-only / empty-body response
        // depend on.
        if self.text.is_none() && self.input.is_empty() {
            return Ok(());
        }
        // Build a Document over `self.input` locally. Doing it here (rather
        // than as part of `XmlDeserializer` state) keeps the iteration
        // borrow scoped to this stack frame, which lets us mutate `self`
        // (via `dispatch_*`) for child-consumer dispatches without fighting
        // a long-lived borrow on `self.state`.
        let input = self.input;
        let mut doc = self.document()?;
        let mut root = doc
            .root_element()
            .map_err(|e| SerdeError::custom(e.to_string()))?;

        // Unwrapped XML output (e.g. S3 `GetBucketLocation` whose body is
        // `<LocationConstraint>...</LocationConstraint>` rather than
        // `<GetBucketLocationOutput><LocationConstraint>...`). The body's
        // root element IS the (sole) member element — dispatch it directly
        // and skip the normal "enter wrapper, iterate children" path. The
        // schema flag is set by codegen for operations with the
        // `S3UnwrappedXmlOutputTrait` AWS customization; non-XML codecs
        // ignore the field, preserving runtime protocol-swap compatibility.
        if schema.xml_unwrapped_output() {
            let local = root.start_el().local().to_owned();
            // Release the iterator borrow on `doc` so we can mutate `self`.
            drop(root);
            drop(doc);
            if let Some(member) = Self::resolve_member(schema, &local) {
                let sub = Self::find_element_slice(input, &local);
                self.dispatch_subslice(sub, None, |this| consumer(member, this))?;
            }
            return Ok(());
        }

        // Dispatch @xmlAttribute members from the start element's attributes.
        for member in schema.members() {
            if member.xml_attribute() {
                let attr_name = member
                    .xml_name()
                    .map(|t| t.value())
                    .or(member.member_name())
                    .unwrap_or("");
                if let Some(value) = root.start_el().attr(attr_name) {
                    let text = Cow::Owned(value.to_owned());
                    self.dispatch_text(text, |this| consumer(member, this))?;
                }
            }
        }

        // Track flattened-aggregate members: their wire format is repeated sibling
        // elements that must be accumulated and dispatched as a single read_list /
        // read_map call. Map: member_index -> (member_schema, accumulated XML bytes).
        let mut flattened_groups: std::collections::HashMap<usize, (&Schema, Vec<u8>)> =
            std::collections::HashMap::new();

        // Dispatch child elements.
        while let Some(mut child_scope) = root.next_tag() {
            let local = child_scope.start_el().local().to_owned();
            let Some(member) = Self::resolve_member(schema, &local) else {
                continue;
            };
            // For non-flattened aggregate members, the child element IS the
            // aggregate container (e.g. `<myList><member>...</member></myList>`).
            // Use a sub-slice into `input` so the consumer's read_list / read_map
            // / read_struct can build its own Document over the child element.
            // For flattened aggregate members, accumulate the bytes of each
            // matching sibling and dispatch them together below.
            // For scalars (including flattened scalars), extract text inline.
            let is_aggregate = member.shape_type().is_aggregate();
            if is_aggregate && !member.xml_flattened() {
                let el_local = child_scope.start_el().local();
                let sub = Self::find_element_slice(input, el_local);
                drop(child_scope);
                self.dispatch_subslice(sub, None, |this| consumer(member, this))?;
            } else if is_aggregate {
                // Flattened aggregate: capture this sibling's slice; dispatch
                // the merged group below.
                let el_local = child_scope.start_el().local();
                let sub = Self::find_element_slice(input, el_local);
                drop(child_scope);
                let idx = member.member_index().unwrap_or(usize::MAX);
                let entry = flattened_groups
                    .entry(idx)
                    .or_insert_with(|| (member, Vec::new()));
                entry.1.extend_from_slice(sub);
            } else {
                let text = decode::try_data(&mut child_scope)
                    .map_err(|e| SerdeError::custom(e.to_string()))?;
                drop(child_scope);
                self.dispatch_text(text, |this| consumer(member, this))?;
            }
        }

        // Dispatch each accumulated flattened-aggregate group as a single call.
        // We synthesize a `<__flat>...</__flat>` wrapper so the consumer's
        // `read_list` / `read_map` sees the collected siblings as
        // wrapper-children and iterates them normally. The wrapper buffer is
        // owned locally (lifetime is shorter than `'a`), so we cannot route
        // it through `dispatch_subslice` — keep a fresh deserializer for
        // this case only.
        for (_idx, (member, bytes)) in flattened_groups {
            let mut wrapped = Vec::with_capacity(bytes.len() + 16);
            wrapped.extend_from_slice(b"<__flat>");
            wrapped.extend_from_slice(&bytes);
            wrapped.extend_from_slice(b"</__flat>");
            let mut child_deser = XmlDeserializer::new(&wrapped, self.settings.clone());
            consumer(member, &mut child_deser)?;
        }
        Ok(())
    }

    fn read_list(
        &mut self,
        _schema: &Schema,
        consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        // Empty input → empty list. See `read_struct` for rationale.
        if self.text.is_none() && self.input.is_empty() {
            return Ok(());
        }
        let input = self.input;
        let mut doc = self.document()?;
        let mut root = doc
            .root_element()
            .map_err(|e| SerdeError::custom(e.to_string()))?;

        // Each child tag is a list item. Provide each item to the consumer
        // by re-pointing `self` at the item's sub-slice via dispatch_subslice.
        // Scalar consumers will navigate to the text via `take_text()`;
        // aggregate consumers (read_list / read_struct / read_map) will
        // descend into the element's content. This keeps the deserializer
        // compatible with both scalar list elements and nested aggregate
        // elements (e.g. list-of-lists, list-of-structs) without per-element
        // type sniffing.
        while let Some(child_scope) = root.next_tag() {
            let el_local = child_scope.start_el().local();
            let sub = Self::find_element_slice(input, el_local);
            drop(child_scope);
            self.dispatch_subslice(sub, None, |this| consumer(this))?;
        }
        Ok(())
    }

    fn read_map(
        &mut self,
        schema: &Schema,
        consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        // If a parent aggregate read installed a `schema_override` on us, it
        // takes priority. Codegen generates inner `read_map` calls reusing
        // the outer `member` schema (closure variable shadowing) — so the
        // arg here is the outer map's schema, not the inner's. The override
        // carries the outer's `_VALUE` schema (which chains the inner map's
        // `_KEY` / `_VALUE`) and is the right one for nested element-name
        // resolution.
        let effective_schema: &Schema =
            self.schema_override.map(|s| s as &Schema).unwrap_or(schema);
        // Once we've read this override, clear it so it doesn't leak to
        // sibling reads on the same deserializer.
        self.schema_override = None;
        let schema = effective_schema;

        // Empty input → empty map. See `read_struct` for rationale.
        if self.text.is_none() && self.input.is_empty() {
            return Ok(());
        }

        let input = self.input;
        let mut doc = self.document()?;
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

        // Aggregate (struct/list/map) value types need a sub-document deserializer
        // since their content includes nested elements rather than just text.
        let value_is_aggregate = schema
            .member()
            .map(|v| v.shape_type().is_aggregate())
            .unwrap_or(false);
        // For aggregate values, save the value member's `'static` schema so a
        // nested inner aggregate read (which codegen invokes with
        // `prelude::DOCUMENT`) can recover its own `_KEY`/`_VALUE`
        // chain via the `schema_override` parameter on `dispatch_subslice`.
        let value_schema_static = schema.member_static();

        // Each child tag is an entry (e.g. <entry><key>k</key><value>v</value></entry>).
        while let Some(mut entry_scope) = root.next_tag() {
            let mut key: Option<String> = None;
            // For scalar values we capture the text upfront; for aggregate
            // values we capture the element sub-slice. At most one is set.
            let mut value_text: Option<Cow<'_, str>> = None;
            let mut value_slice: Option<&'_ [u8]> = None;
            while let Some(mut field_scope) = entry_scope.next_tag() {
                let local = field_scope.start_el().local().to_owned();
                if local == key_name {
                    let text = decode::try_data(&mut field_scope)
                        .map_err(|e| SerdeError::custom(e.to_string()))?;
                    key = Some(text.into_owned());
                } else if local == value_name {
                    if value_is_aggregate {
                        let el_local = field_scope.start_el().local();
                        let sub = Self::find_element_slice(input, el_local);
                        drop(field_scope);
                        value_slice = Some(sub);
                    } else {
                        let text = decode::try_data(&mut field_scope)
                            .map_err(|e| SerdeError::custom(e.to_string()))?;
                        value_text = Some(text);
                    }
                }
            }
            drop(entry_scope);
            if let Some(k) = key {
                if let Some(slice) = value_slice {
                    self.dispatch_subslice(slice, value_schema_static, |this| consumer(k, this))?;
                } else if let Some(t) = value_text {
                    // Re-borrow t as 'a — the text was extracted from `doc`
                    // which borrows `self.input: &'a [u8]`, so its lifetime
                    // is `'a`.
                    let t: Cow<'_, str> = t;
                    self.dispatch_text(t.into_owned().into(), |this| consumer(k, this))?;
                }
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
        // Verify a Doc-state deserializer can extract leaf text via the
        // public `read_string` API (which goes through `take_text` →
        // lazy Document construction over `self.input`).
        static V_MEMBER: Schema =
            Schema::new_member(shape_id!("test", "X$v"), ShapeType::String, "v", 0);
        let xml = b"<root>world</root>";
        let settings = Arc::new(XmlCodecSettings::default());
        let mut deser = XmlDeserializer::new(xml, settings);
        let result = deser.read_string(&V_MEMBER).unwrap();
        assert_eq!(result, "world");
    }

    #[test]
    fn is_null_always_false() {
        let settings = Arc::new(XmlCodecSettings::default());
        let deser = XmlDeserializer::new(b"<r/>", settings);
        assert!(!deser.is_null());
    }

    // Struct member dispatch by element name (`@xmlName` and member name).

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

    // `@xmlAttribute` dispatch from the start element's attributes.

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

    // Wrapped list / map reads with element-name resolution.

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

    // Flattened collections: repeated sibling elements accumulated into one read.

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
                    "items" => d.read_list(member, &mut |d| {
                        items.push(d.read_string(member)?);
                        Ok(())
                    })?,
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
                    "items" => d.read_list(member, &mut |d| {
                        items.push(d.read_string(member)?);
                        Ok(())
                    })?,
                    _ => {}
                }
                Ok(())
            })
            .unwrap();

        assert_eq!(name, "n");
        assert_eq!(items, vec!["x", "y"]);
    }

    // Scalar reads (booleans, ints, floats, blob, timestamp) and document rejection.

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

    // Empty body → empty struct/list/map. Required by S3 HEAD operations and
    // any restXml response whose Output members are entirely HTTP-bound
    // (headers, status code) with no body content.
    #[test]
    fn read_struct_empty_body_is_empty_struct() {
        let settings = Arc::new(XmlCodecSettings::default());
        let mut deser = XmlDeserializer::new(b"", settings);
        let mut called = false;
        deser
            .read_struct(&PERSON_SCHEMA, &mut |_member, _d| {
                called = true;
                Ok(())
            })
            .expect("empty body should be accepted as empty struct");
        assert!(!called, "consumer must not run for empty input");
    }

    #[test]
    fn read_list_empty_body_is_empty_list() {
        let settings = Arc::new(XmlCodecSettings::default());
        let mut deser = XmlDeserializer::new(b"", settings);
        let mut called = false;
        deser
            .read_list(&PERSON_SCHEMA, &mut |_d| {
                called = true;
                Ok(())
            })
            .expect("empty body should be accepted as empty list");
        assert!(!called, "consumer must not run for empty input");
    }

    #[test]
    fn read_map_empty_body_is_empty_map() {
        let settings = Arc::new(XmlCodecSettings::default());
        let mut deser = XmlDeserializer::new(b"", settings);
        let mut called = false;
        deser
            .read_map(&PERSON_SCHEMA, &mut |_k, _d| {
                called = true;
                Ok(())
            })
            .expect("empty body should be accepted as empty map");
        assert!(!called, "consumer must not run for empty input");
    }
}
