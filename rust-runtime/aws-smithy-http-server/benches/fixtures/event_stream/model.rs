/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! One-time snapshot of generated event-stream benchmark SDK code. See README.md.
#![allow(dead_code)]

#[allow(missing_docs)] // documentation missing in model
#[derive(::std::clone::Clone, ::std::cmp::PartialEq, ::std::fmt::Debug)]
pub enum Events {
    #[allow(missing_docs)] // documentation missing in model
    Binary(crate::model::BinaryEvent),
    #[allow(missing_docs)] // documentation missing in model
    Structured(crate::model::StructuredEvent),
}
impl Events {
    /// Tries to convert the enum instance into [`Binary`](crate::model::Events::Binary), extracting the inner [`BinaryEvent`](crate::model::BinaryEvent).
    /// Returns `Err(&Self)` if it can't be converted.
    pub fn as_binary(&self) -> ::std::result::Result<&crate::model::BinaryEvent, &Self> {
        if let Events::Binary(val) = &self {
            ::std::result::Result::Ok(val)
        } else {
            ::std::result::Result::Err(self)
        }
    }
    /// Returns true if this is a [`Binary`](crate::model::Events::Binary).
    pub fn is_binary(&self) -> bool {
        self.as_binary().is_ok()
    }
    /// Tries to convert the enum instance into [`Structured`](crate::model::Events::Structured), extracting the inner [`StructuredEvent`](crate::model::StructuredEvent).
    /// Returns `Err(&Self)` if it can't be converted.
    pub fn as_structured(&self) -> ::std::result::Result<&crate::model::StructuredEvent, &Self> {
        if let Events::Structured(val) = &self {
            ::std::result::Result::Ok(val)
        } else {
            ::std::result::Result::Err(self)
        }
    }
    /// Returns true if this is a [`Structured`](crate::model::Events::Structured).
    pub fn is_structured(&self) -> bool {
        self.as_structured().is_ok()
    }
}
static EVENTS_MEMBER_STRUCTURED: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_member(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#Events$structured",
        "benchmark.eventstream",
        "Events",
    ),
    ::aws_smithy_schema::ShapeType::Structure,
    "structured",
    0,
);
static EVENTS_MEMBER_BINARY: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_member(
    ::aws_smithy_schema::ShapeId::from_parts("benchmark.eventstream#Events$binary", "benchmark.eventstream", "Events"),
    ::aws_smithy_schema::ShapeType::Structure,
    "binary",
    1,
);
static EVENTS_SCHEMA: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_struct(
    ::aws_smithy_schema::ShapeId::from_parts("benchmark.eventstream#Events", "benchmark.eventstream", "Events"),
    ::aws_smithy_schema::ShapeType::Union,
    &[&EVENTS_MEMBER_STRUCTURED, &EVENTS_MEMBER_BINARY],
)
.with_streaming();
impl crate::model::Events {
    /// The schema for this shape.
    pub const SCHEMA: &'static ::aws_smithy_schema::Schema<'static> = &EVENTS_SCHEMA;
}
impl ::aws_smithy_schema::serde::SerializableStruct for crate::model::Events {
    fn schema(&self) -> &::aws_smithy_schema::Schema<'_> {
        Self::SCHEMA
    }

    #[allow(unused_variables, clippy::diverging_sub_expression)]
    fn serialize_members(
        &self,
        ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer,
    ) -> ::std::result::Result<(), ::aws_smithy_schema::serde::SerdeError> {
        match self {
            Self::Structured(val) => {
                ser.write_struct(&EVENTS_MEMBER_STRUCTURED, val)?;
            }
            Self::Binary(val) => {
                ser.write_struct(&EVENTS_MEMBER_BINARY, val)?;
            }
        }
        Ok(())
    }
}

#[allow(missing_docs)] // documentation missing in model
#[derive(::std::clone::Clone, ::std::cmp::Eq, ::std::cmp::PartialEq, ::std::fmt::Debug, ::std::hash::Hash)]
pub struct StructuredEvent {
    #[allow(missing_docs)] // documentation missing in model
    pub sequence: ::std::option::Option<i64>,
    #[allow(missing_docs)] // documentation missing in model
    pub payload: ::std::option::Option<crate::model::StructuredData>,
}
impl StructuredEvent {
    #[allow(missing_docs)] // documentation missing in model
    pub fn sequence(&self) -> ::std::option::Option<i64> {
        self.sequence
    }
    #[allow(missing_docs)] // documentation missing in model
    pub fn payload(&self) -> ::std::option::Option<&crate::model::StructuredData> {
        self.payload.as_ref()
    }
}
static STRUCTUREDEVENT_MEMBER_SEQUENCE: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_member(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#StructuredEvent$sequence",
        "benchmark.eventstream",
        "StructuredEvent",
    ),
    ::aws_smithy_schema::ShapeType::Long,
    "sequence",
    0,
)
.with_event_header();
static STRUCTUREDEVENT_MEMBER_PAYLOAD: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_member(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#StructuredEvent$payload",
        "benchmark.eventstream",
        "StructuredEvent",
    ),
    ::aws_smithy_schema::ShapeType::Structure,
    "payload",
    1,
)
.with_event_payload();
static STRUCTUREDEVENT_SCHEMA: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_struct(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#StructuredEvent",
        "benchmark.eventstream",
        "StructuredEvent",
    ),
    ::aws_smithy_schema::ShapeType::Structure,
    &[&STRUCTUREDEVENT_MEMBER_SEQUENCE, &STRUCTUREDEVENT_MEMBER_PAYLOAD],
);
impl crate::model::StructuredEvent {
    /// The schema for this shape.
    pub const SCHEMA: &'static ::aws_smithy_schema::Schema<'static> = &STRUCTUREDEVENT_SCHEMA;
}
impl ::aws_smithy_schema::serde::SerializableStruct for crate::model::StructuredEvent {
    fn schema(&self) -> &::aws_smithy_schema::Schema<'_> {
        Self::SCHEMA
    }

    #[allow(unused_variables, clippy::diverging_sub_expression)]
    fn serialize_members(
        &self,
        ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer,
    ) -> ::std::result::Result<(), ::aws_smithy_schema::serde::SerdeError> {
        if let Some(ref val) = self.sequence {
            ser.write_long(&STRUCTUREDEVENT_MEMBER_SEQUENCE, *val)?;
        }
        if let Some(ref val) = self.payload {
            ser.write_struct(&STRUCTUREDEVENT_MEMBER_PAYLOAD, val)?;
        }
        Ok(())
    }
}
impl StructuredEvent {
    /// Reads this shape from a [`ShapeDeserializer`](::aws_smithy_schema::serde::ShapeDeserializer) guided by [`Self::SCHEMA`].
    ///
    /// Returns the shape.
    #[allow(
        dead_code,
        unused_variables,
        unused_mut,
        clippy::match_single_binding,
        clippy::single_match
    )]
    pub(crate) fn deserialize_schema(
        deserializer: &mut dyn ::aws_smithy_schema::serde::ShapeDeserializer,
    ) -> ::std::result::Result<crate::model::StructuredEvent, ::aws_smithy_schema::serde::SerdeError> {
        let mut builder = crate::model::structured_event::Builder::default();
        deserializer.read_struct(Self::SCHEMA, &mut |member, deser| {
            match member.member_index() {
                Some(0) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.sequence = Some(deser.read_long(member)?);
                    }
                }
                Some(1) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.payload = Some(crate::model::StructuredData::deserialize_schema(deser)?);
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        Ok(builder.build())
    }
}
impl StructuredEvent {
    /// Reads the document members of this shape from a [`ShapeDeserializer`](::aws_smithy_schema::serde::ShapeDeserializer)
    /// guided by [`Self::SCHEMA`] into `builder`.
    #[allow(
        dead_code,
        unused_variables,
        unused_mut,
        clippy::match_single_binding,
        clippy::single_match
    )]
    pub(crate) fn deserialize_schema_into(
        builder: &mut crate::model::structured_event::Builder,
        deserializer: &mut dyn ::aws_smithy_schema::serde::ShapeDeserializer,
    ) -> ::std::result::Result<(), ::aws_smithy_schema::serde::SerdeError> {
        deserializer.read_struct(Self::SCHEMA, &mut |member, deser| {
            match member.member_index() {
                Some(0) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.sequence = Some(deser.read_long(member)?);
                    }
                }
                Some(1) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.payload = Some(crate::model::StructuredData::deserialize_schema(deser)?);
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        Ok(())
    }
}
impl StructuredEvent {
    /// Creates a new builder-style object to manufacture [`StructuredEvent`](crate::model::StructuredEvent).
    pub fn builder() -> crate::model::structured_event::Builder {
        crate::model::structured_event::Builder::default()
    }
}

#[allow(missing_docs)] // documentation missing in model
#[derive(::std::clone::Clone, ::std::cmp::Eq, ::std::cmp::PartialEq, ::std::fmt::Debug, ::std::hash::Hash)]
pub struct BinaryEvent {
    #[allow(missing_docs)] // documentation missing in model
    pub sequence: ::std::option::Option<i64>,
    #[allow(missing_docs)] // documentation missing in model
    pub payload: ::std::option::Option<::aws_smithy_types::Blob>,
}
impl BinaryEvent {
    #[allow(missing_docs)] // documentation missing in model
    pub fn sequence(&self) -> ::std::option::Option<i64> {
        self.sequence
    }
    #[allow(missing_docs)] // documentation missing in model
    pub fn payload(&self) -> ::std::option::Option<&::aws_smithy_types::Blob> {
        self.payload.as_ref()
    }
}
static BINARYEVENT_MEMBER_SEQUENCE: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_member(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#BinaryEvent$sequence",
        "benchmark.eventstream",
        "BinaryEvent",
    ),
    ::aws_smithy_schema::ShapeType::Long,
    "sequence",
    0,
)
.with_event_header();
static BINARYEVENT_MEMBER_PAYLOAD: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_member(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#BinaryEvent$payload",
        "benchmark.eventstream",
        "BinaryEvent",
    ),
    ::aws_smithy_schema::ShapeType::Blob,
    "payload",
    1,
)
.with_event_payload();
static BINARYEVENT_SCHEMA: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_struct(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#BinaryEvent",
        "benchmark.eventstream",
        "BinaryEvent",
    ),
    ::aws_smithy_schema::ShapeType::Structure,
    &[&BINARYEVENT_MEMBER_SEQUENCE, &BINARYEVENT_MEMBER_PAYLOAD],
);
impl crate::model::BinaryEvent {
    /// The schema for this shape.
    pub const SCHEMA: &'static ::aws_smithy_schema::Schema<'static> = &BINARYEVENT_SCHEMA;
}
impl ::aws_smithy_schema::serde::SerializableStruct for crate::model::BinaryEvent {
    fn schema(&self) -> &::aws_smithy_schema::Schema<'_> {
        Self::SCHEMA
    }

    #[allow(unused_variables, clippy::diverging_sub_expression)]
    fn serialize_members(
        &self,
        ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer,
    ) -> ::std::result::Result<(), ::aws_smithy_schema::serde::SerdeError> {
        if let Some(ref val) = self.sequence {
            ser.write_long(&BINARYEVENT_MEMBER_SEQUENCE, *val)?;
        }
        if let Some(ref val) = self.payload {
            ser.write_blob(&BINARYEVENT_MEMBER_PAYLOAD, val.clone())?;
        }
        Ok(())
    }
}
impl BinaryEvent {
    /// Reads this shape from a [`ShapeDeserializer`](::aws_smithy_schema::serde::ShapeDeserializer) guided by [`Self::SCHEMA`].
    ///
    /// Returns the shape.
    #[allow(
        dead_code,
        unused_variables,
        unused_mut,
        clippy::match_single_binding,
        clippy::single_match
    )]
    pub(crate) fn deserialize_schema(
        deserializer: &mut dyn ::aws_smithy_schema::serde::ShapeDeserializer,
    ) -> ::std::result::Result<crate::model::BinaryEvent, ::aws_smithy_schema::serde::SerdeError> {
        let mut builder = crate::model::binary_event::Builder::default();
        deserializer.read_struct(Self::SCHEMA, &mut |member, deser| {
            match member.member_index() {
                Some(0) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.sequence = Some(deser.read_long(member)?);
                    }
                }
                Some(1) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.payload = Some(deser.read_blob(member)?);
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        Ok(builder.build())
    }
}
impl BinaryEvent {
    /// Reads the document members of this shape from a [`ShapeDeserializer`](::aws_smithy_schema::serde::ShapeDeserializer)
    /// guided by [`Self::SCHEMA`] into `builder`.
    #[allow(
        dead_code,
        unused_variables,
        unused_mut,
        clippy::match_single_binding,
        clippy::single_match
    )]
    pub(crate) fn deserialize_schema_into(
        builder: &mut crate::model::binary_event::Builder,
        deserializer: &mut dyn ::aws_smithy_schema::serde::ShapeDeserializer,
    ) -> ::std::result::Result<(), ::aws_smithy_schema::serde::SerdeError> {
        deserializer.read_struct(Self::SCHEMA, &mut |member, deser| {
            match member.member_index() {
                Some(0) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.sequence = Some(deser.read_long(member)?);
                    }
                }
                Some(1) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.payload = Some(deser.read_blob(member)?);
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        Ok(())
    }
}
impl BinaryEvent {
    /// Creates a new builder-style object to manufacture [`BinaryEvent`](crate::model::BinaryEvent).
    pub fn builder() -> crate::model::binary_event::Builder {
        crate::model::binary_event::Builder::default()
    }
}

#[allow(missing_docs)] // documentation missing in model
#[derive(::std::clone::Clone, ::std::cmp::Eq, ::std::cmp::PartialEq, ::std::fmt::Debug, ::std::hash::Hash)]
pub struct StructuredData {
    #[allow(missing_docs)] // documentation missing in model
    pub text: ::std::option::Option<::std::string::String>,
    #[allow(missing_docs)] // documentation missing in model
    pub nested: ::std::option::Option<crate::model::Nested>,
    #[allow(missing_docs)] // documentation missing in model
    pub values: ::std::option::Option<::std::vec::Vec<i32>>,
}
impl StructuredData {
    #[allow(missing_docs)] // documentation missing in model
    pub fn text(&self) -> ::std::option::Option<&str> {
        self.text.as_deref()
    }
    #[allow(missing_docs)] // documentation missing in model
    pub fn nested(&self) -> ::std::option::Option<&crate::model::Nested> {
        self.nested.as_ref()
    }
    #[allow(missing_docs)] // documentation missing in model
    pub fn values(&self) -> ::std::option::Option<&[i32]> {
        self.values.as_deref()
    }
}
static STRUCTUREDDATA_MEMBER_TEXT: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_member(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#StructuredData$text",
        "benchmark.eventstream",
        "StructuredData",
    ),
    ::aws_smithy_schema::ShapeType::String,
    "text",
    0,
);
static STRUCTUREDDATA_MEMBER_NESTED: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_member(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#StructuredData$nested",
        "benchmark.eventstream",
        "StructuredData",
    ),
    ::aws_smithy_schema::ShapeType::Structure,
    "nested",
    1,
);
static STRUCTUREDDATA_MEMBER_VALUES_MEMBER: ::aws_smithy_schema::Schema<'static> =
    ::aws_smithy_schema::Schema::new_member(
        ::aws_smithy_schema::ShapeId::from_parts(
            "benchmark.eventstream#Values$member",
            "benchmark.eventstream",
            "Values",
        ),
        ::aws_smithy_schema::ShapeType::Integer,
        "member",
        0,
    );
static STRUCTUREDDATA_MEMBER_VALUES: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_member(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#StructuredData$values",
        "benchmark.eventstream",
        "StructuredData",
    ),
    ::aws_smithy_schema::ShapeType::List,
    "values",
    2,
)
.with_list_member(&STRUCTUREDDATA_MEMBER_VALUES_MEMBER);
static STRUCTUREDDATA_SCHEMA: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_struct(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#StructuredData",
        "benchmark.eventstream",
        "StructuredData",
    ),
    ::aws_smithy_schema::ShapeType::Structure,
    &[
        &STRUCTUREDDATA_MEMBER_TEXT,
        &STRUCTUREDDATA_MEMBER_NESTED,
        &STRUCTUREDDATA_MEMBER_VALUES,
    ],
);
impl crate::model::StructuredData {
    /// The schema for this shape.
    pub const SCHEMA: &'static ::aws_smithy_schema::Schema<'static> = &STRUCTUREDDATA_SCHEMA;
}
impl ::aws_smithy_schema::serde::SerializableStruct for crate::model::StructuredData {
    fn schema(&self) -> &::aws_smithy_schema::Schema<'_> {
        Self::SCHEMA
    }

    #[allow(unused_variables, clippy::diverging_sub_expression)]
    fn serialize_members(
        &self,
        ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer,
    ) -> ::std::result::Result<(), ::aws_smithy_schema::serde::SerdeError> {
        if let Some(ref val) = self.text {
            ser.write_string(&STRUCTUREDDATA_MEMBER_TEXT, val)?;
        }
        if let Some(ref val) = self.nested {
            ser.write_struct(&STRUCTUREDDATA_MEMBER_NESTED, val)?;
        }
        if let Some(ref val) = self.values {
            ser.write_integer_list(&STRUCTUREDDATA_MEMBER_VALUES, val)?;
        }
        Ok(())
    }
}
impl StructuredData {
    /// Reads this shape from a [`ShapeDeserializer`](::aws_smithy_schema::serde::ShapeDeserializer) guided by [`Self::SCHEMA`].
    ///
    /// Returns the shape.
    #[allow(
        dead_code,
        unused_variables,
        unused_mut,
        clippy::match_single_binding,
        clippy::single_match
    )]
    pub(crate) fn deserialize_schema(
        deserializer: &mut dyn ::aws_smithy_schema::serde::ShapeDeserializer,
    ) -> ::std::result::Result<crate::model::StructuredData, ::aws_smithy_schema::serde::SerdeError> {
        let mut builder = crate::model::structured_data::Builder::default();
        deserializer.read_struct(Self::SCHEMA, &mut |member, deser| {
            match member.member_index() {
                Some(0) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.text = Some(deser.read_string(member)?);
                    }
                }
                Some(1) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.nested = Some(crate::model::Nested::deserialize_schema(deser)?);
                    }
                }
                Some(2) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.values = Some(deser.read_integer_list(member)?);
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        Ok(builder.build())
    }
}
impl StructuredData {
    /// Reads the document members of this shape from a [`ShapeDeserializer`](::aws_smithy_schema::serde::ShapeDeserializer)
    /// guided by [`Self::SCHEMA`] into `builder`.
    #[allow(
        dead_code,
        unused_variables,
        unused_mut,
        clippy::match_single_binding,
        clippy::single_match
    )]
    pub(crate) fn deserialize_schema_into(
        builder: &mut crate::model::structured_data::Builder,
        deserializer: &mut dyn ::aws_smithy_schema::serde::ShapeDeserializer,
    ) -> ::std::result::Result<(), ::aws_smithy_schema::serde::SerdeError> {
        deserializer.read_struct(Self::SCHEMA, &mut |member, deser| {
            match member.member_index() {
                Some(0) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.text = Some(deser.read_string(member)?);
                    }
                }
                Some(1) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.nested = Some(crate::model::Nested::deserialize_schema(deser)?);
                    }
                }
                Some(2) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.values = Some(deser.read_integer_list(member)?);
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        Ok(())
    }
}
impl StructuredData {
    /// Creates a new builder-style object to manufacture [`StructuredData`](crate::model::StructuredData).
    pub fn builder() -> crate::model::structured_data::Builder {
        crate::model::structured_data::Builder::default()
    }
}

#[allow(missing_docs)] // documentation missing in model
#[derive(::std::clone::Clone, ::std::cmp::Eq, ::std::cmp::PartialEq, ::std::fmt::Debug, ::std::hash::Hash)]
pub struct Nested {
    #[allow(missing_docs)] // documentation missing in model
    pub label: ::std::option::Option<::std::string::String>,
    #[allow(missing_docs)] // documentation missing in model
    pub enabled: ::std::option::Option<bool>,
}
impl Nested {
    #[allow(missing_docs)] // documentation missing in model
    pub fn label(&self) -> ::std::option::Option<&str> {
        self.label.as_deref()
    }
    #[allow(missing_docs)] // documentation missing in model
    pub fn enabled(&self) -> ::std::option::Option<bool> {
        self.enabled
    }
}
static NESTED_MEMBER_LABEL: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_member(
    ::aws_smithy_schema::ShapeId::from_parts("benchmark.eventstream#Nested$label", "benchmark.eventstream", "Nested"),
    ::aws_smithy_schema::ShapeType::String,
    "label",
    0,
);
static NESTED_MEMBER_ENABLED: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_member(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#Nested$enabled",
        "benchmark.eventstream",
        "Nested",
    ),
    ::aws_smithy_schema::ShapeType::Boolean,
    "enabled",
    1,
);
static NESTED_SCHEMA: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_struct(
    ::aws_smithy_schema::ShapeId::from_parts("benchmark.eventstream#Nested", "benchmark.eventstream", "Nested"),
    ::aws_smithy_schema::ShapeType::Structure,
    &[&NESTED_MEMBER_LABEL, &NESTED_MEMBER_ENABLED],
);
impl crate::model::Nested {
    /// The schema for this shape.
    pub const SCHEMA: &'static ::aws_smithy_schema::Schema<'static> = &NESTED_SCHEMA;
}
impl ::aws_smithy_schema::serde::SerializableStruct for crate::model::Nested {
    fn schema(&self) -> &::aws_smithy_schema::Schema<'_> {
        Self::SCHEMA
    }

    #[allow(unused_variables, clippy::diverging_sub_expression)]
    fn serialize_members(
        &self,
        ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer,
    ) -> ::std::result::Result<(), ::aws_smithy_schema::serde::SerdeError> {
        if let Some(ref val) = self.label {
            ser.write_string(&NESTED_MEMBER_LABEL, val)?;
        }
        if let Some(ref val) = self.enabled {
            ser.write_boolean(&NESTED_MEMBER_ENABLED, *val)?;
        }
        Ok(())
    }
}
impl Nested {
    /// Reads this shape from a [`ShapeDeserializer`](::aws_smithy_schema::serde::ShapeDeserializer) guided by [`Self::SCHEMA`].
    ///
    /// Returns the shape.
    #[allow(
        dead_code,
        unused_variables,
        unused_mut,
        clippy::match_single_binding,
        clippy::single_match
    )]
    pub(crate) fn deserialize_schema(
        deserializer: &mut dyn ::aws_smithy_schema::serde::ShapeDeserializer,
    ) -> ::std::result::Result<crate::model::Nested, ::aws_smithy_schema::serde::SerdeError> {
        let mut builder = crate::model::nested::Builder::default();
        deserializer.read_struct(Self::SCHEMA, &mut |member, deser| {
            match member.member_index() {
                Some(0) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.label = Some(deser.read_string(member)?);
                    }
                }
                Some(1) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.enabled = Some(deser.read_boolean(member)?);
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        Ok(builder.build())
    }
}
impl Nested {
    /// Reads the document members of this shape from a [`ShapeDeserializer`](::aws_smithy_schema::serde::ShapeDeserializer)
    /// guided by [`Self::SCHEMA`] into `builder`.
    #[allow(
        dead_code,
        unused_variables,
        unused_mut,
        clippy::match_single_binding,
        clippy::single_match
    )]
    pub(crate) fn deserialize_schema_into(
        builder: &mut crate::model::nested::Builder,
        deserializer: &mut dyn ::aws_smithy_schema::serde::ShapeDeserializer,
    ) -> ::std::result::Result<(), ::aws_smithy_schema::serde::SerdeError> {
        deserializer.read_struct(Self::SCHEMA, &mut |member, deser| {
            match member.member_index() {
                Some(0) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.label = Some(deser.read_string(member)?);
                    }
                }
                Some(1) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.enabled = Some(deser.read_boolean(member)?);
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        Ok(())
    }
}
impl Nested {
    /// Creates a new builder-style object to manufacture [`Nested`](crate::model::Nested).
    pub fn builder() -> crate::model::nested::Builder {
        crate::model::nested::Builder::default()
    }
}

/// See [`StructuredEvent`](crate::model::StructuredEvent).
pub mod structured_event {

    impl ::std::convert::From<Builder> for crate::model::StructuredEvent {
        fn from(builder: Builder) -> Self {
            builder.build()
        }
    }
    /// A builder for [`StructuredEvent`](crate::model::StructuredEvent).
    #[derive(::std::clone::Clone, ::std::default::Default, ::std::fmt::Debug)]
    pub struct Builder {
        pub(crate) sequence: ::std::option::Option<i64>,
        pub(crate) payload: ::std::option::Option<crate::model::StructuredData>,
    }
    impl Builder {
        #[allow(missing_docs)] // documentation missing in model
        pub fn sequence(mut self, input: ::std::option::Option<i64>) -> Self {
            self.sequence = input;
            self
        }
        #[allow(missing_docs)] // documentation missing in model
        #[allow(dead_code)]
        pub(crate) fn set_sequence(mut self, input: Option<impl ::std::convert::Into<i64>>) -> Self {
            self.sequence = input.map(|v| v.into());
            self
        }
        #[allow(missing_docs)] // documentation missing in model
        pub fn payload(mut self, input: ::std::option::Option<crate::model::StructuredData>) -> Self {
            self.payload = input;
            self
        }
        #[allow(missing_docs)] // documentation missing in model
        #[allow(dead_code)]
        pub(crate) fn set_payload(
            mut self,
            input: Option<impl ::std::convert::Into<crate::model::StructuredData>>,
        ) -> Self {
            self.payload = input.map(|v| v.into());
            self
        }
        /// Consumes the builder and constructs a [`StructuredEvent`](crate::model::StructuredEvent).
        pub fn build(self) -> crate::model::StructuredEvent {
            self.build_enforcing_all_constraints()
        }
        fn build_enforcing_all_constraints(self) -> crate::model::StructuredEvent {
            crate::model::StructuredEvent {
                sequence: self.sequence,
                payload: self.payload,
            }
        }
    }
}
/// See [`BinaryEvent`](crate::model::BinaryEvent).
pub mod binary_event {

    impl ::std::convert::From<Builder> for crate::model::BinaryEvent {
        fn from(builder: Builder) -> Self {
            builder.build()
        }
    }
    /// A builder for [`BinaryEvent`](crate::model::BinaryEvent).
    #[derive(::std::clone::Clone, ::std::default::Default, ::std::fmt::Debug)]
    pub struct Builder {
        pub(crate) sequence: ::std::option::Option<i64>,
        pub(crate) payload: ::std::option::Option<::aws_smithy_types::Blob>,
    }
    impl Builder {
        #[allow(missing_docs)] // documentation missing in model
        pub fn sequence(mut self, input: ::std::option::Option<i64>) -> Self {
            self.sequence = input;
            self
        }
        #[allow(missing_docs)] // documentation missing in model
        #[allow(dead_code)]
        pub(crate) fn set_sequence(mut self, input: Option<impl ::std::convert::Into<i64>>) -> Self {
            self.sequence = input.map(|v| v.into());
            self
        }
        #[allow(missing_docs)] // documentation missing in model
        pub fn payload(mut self, input: ::std::option::Option<::aws_smithy_types::Blob>) -> Self {
            self.payload = input;
            self
        }
        #[allow(missing_docs)] // documentation missing in model
        #[allow(dead_code)]
        pub(crate) fn set_payload(
            mut self,
            input: Option<impl ::std::convert::Into<::aws_smithy_types::Blob>>,
        ) -> Self {
            self.payload = input.map(|v| v.into());
            self
        }
        /// Consumes the builder and constructs a [`BinaryEvent`](crate::model::BinaryEvent).
        pub fn build(self) -> crate::model::BinaryEvent {
            self.build_enforcing_all_constraints()
        }
        fn build_enforcing_all_constraints(self) -> crate::model::BinaryEvent {
            crate::model::BinaryEvent {
                sequence: self.sequence,
                payload: self.payload,
            }
        }
    }
}
/// See [`StructuredData`](crate::model::StructuredData).
pub mod structured_data {

    impl ::std::convert::From<Builder> for crate::model::StructuredData {
        fn from(builder: Builder) -> Self {
            builder.build()
        }
    }
    /// A builder for [`StructuredData`](crate::model::StructuredData).
    #[derive(::std::clone::Clone, ::std::default::Default, ::std::fmt::Debug)]
    pub struct Builder {
        pub(crate) text: ::std::option::Option<::std::string::String>,
        pub(crate) nested: ::std::option::Option<crate::model::Nested>,
        pub(crate) values: ::std::option::Option<::std::vec::Vec<i32>>,
    }
    impl Builder {
        #[allow(missing_docs)] // documentation missing in model
        pub fn text(mut self, input: ::std::option::Option<::std::string::String>) -> Self {
            self.text = input;
            self
        }
        #[allow(missing_docs)] // documentation missing in model
        #[allow(dead_code)]
        pub(crate) fn set_text(mut self, input: Option<impl ::std::convert::Into<::std::string::String>>) -> Self {
            self.text = input.map(|v| v.into());
            self
        }
        #[allow(missing_docs)] // documentation missing in model
        pub fn nested(mut self, input: ::std::option::Option<crate::model::Nested>) -> Self {
            self.nested = input;
            self
        }
        #[allow(missing_docs)] // documentation missing in model
        #[allow(dead_code)]
        pub(crate) fn set_nested(mut self, input: Option<impl ::std::convert::Into<crate::model::Nested>>) -> Self {
            self.nested = input.map(|v| v.into());
            self
        }
        #[allow(missing_docs)] // documentation missing in model
        pub fn values(mut self, input: ::std::option::Option<::std::vec::Vec<i32>>) -> Self {
            self.values = input;
            self
        }
        #[allow(missing_docs)] // documentation missing in model
        #[allow(dead_code)]
        pub(crate) fn set_values(mut self, input: Option<impl ::std::convert::Into<::std::vec::Vec<i32>>>) -> Self {
            self.values = input.map(|v| v.into());
            self
        }
        /// Consumes the builder and constructs a [`StructuredData`](crate::model::StructuredData).
        pub fn build(self) -> crate::model::StructuredData {
            self.build_enforcing_all_constraints()
        }
        fn build_enforcing_all_constraints(self) -> crate::model::StructuredData {
            crate::model::StructuredData {
                text: self.text,
                nested: self.nested,
                values: self.values,
            }
        }
    }
}
/// See [`Nested`](crate::model::Nested).
pub mod nested {

    impl ::std::convert::From<Builder> for crate::model::Nested {
        fn from(builder: Builder) -> Self {
            builder.build()
        }
    }
    /// A builder for [`Nested`](crate::model::Nested).
    #[derive(::std::clone::Clone, ::std::default::Default, ::std::fmt::Debug)]
    pub struct Builder {
        pub(crate) label: ::std::option::Option<::std::string::String>,
        pub(crate) enabled: ::std::option::Option<bool>,
    }
    impl Builder {
        #[allow(missing_docs)] // documentation missing in model
        pub fn label(mut self, input: ::std::option::Option<::std::string::String>) -> Self {
            self.label = input;
            self
        }
        #[allow(missing_docs)] // documentation missing in model
        #[allow(dead_code)]
        pub(crate) fn set_label(mut self, input: Option<impl ::std::convert::Into<::std::string::String>>) -> Self {
            self.label = input.map(|v| v.into());
            self
        }
        #[allow(missing_docs)] // documentation missing in model
        pub fn enabled(mut self, input: ::std::option::Option<bool>) -> Self {
            self.enabled = input;
            self
        }
        #[allow(missing_docs)] // documentation missing in model
        #[allow(dead_code)]
        pub(crate) fn set_enabled(mut self, input: Option<impl ::std::convert::Into<bool>>) -> Self {
            self.enabled = input.map(|v| v.into());
            self
        }
        /// Consumes the builder and constructs a [`Nested`](crate::model::Nested).
        pub fn build(self) -> crate::model::Nested {
            self.build_enforcing_all_constraints()
        }
        fn build_enforcing_all_constraints(self) -> crate::model::Nested {
            crate::model::Nested {
                label: self.label,
                enabled: self.enabled,
            }
        }
    }
}
