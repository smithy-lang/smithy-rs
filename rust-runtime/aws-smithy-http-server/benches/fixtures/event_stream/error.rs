/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! One-time snapshot of generated event-stream benchmark SDK code. See README.md.
#![allow(dead_code)]

#[allow(missing_docs)] // documentation missing in model
#[derive(::std::clone::Clone, ::std::cmp::Eq, ::std::cmp::PartialEq, ::std::fmt::Debug, ::std::hash::Hash)]
pub struct StreamFailure {
    #[allow(missing_docs)] // documentation missing in model
    pub message: ::std::option::Option<::std::string::String>,
}
static STREAMFAILURE_MEMBER_MESSAGE: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_member(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#StreamFailure$message",
        "benchmark.eventstream",
        "StreamFailure",
    ),
    ::aws_smithy_schema::ShapeType::String,
    "message",
    0,
);
static STREAMFAILURE_TRAITS: std::sync::LazyLock<::aws_smithy_schema::TraitMap> = std::sync::LazyLock::new(|| {
    let mut map = ::aws_smithy_schema::TraitMap::new();
    map.insert(::std::boxed::Box::new(::aws_smithy_schema::StringTrait::new(
        ::aws_smithy_schema::ShapeId::from_parts("smithy.api#error", "smithy.api", "error"),
        "server",
    )));
    map
});
static STREAMFAILURE_SCHEMA: ::aws_smithy_schema::Schema<'static> = ::aws_smithy_schema::Schema::new_struct(
    ::aws_smithy_schema::ShapeId::from_parts(
        "benchmark.eventstream#StreamFailure",
        "benchmark.eventstream",
        "StreamFailure",
    ),
    ::aws_smithy_schema::ShapeType::Structure,
    &[&STREAMFAILURE_MEMBER_MESSAGE],
)
.with_traits(&STREAMFAILURE_TRAITS);
impl crate::error::StreamFailure {
    /// The schema for this shape.
    pub const SCHEMA: &'static ::aws_smithy_schema::Schema<'static> = &STREAMFAILURE_SCHEMA;
}
impl ::aws_smithy_schema::serde::SerializableStruct for crate::error::StreamFailure {
    fn schema(&self) -> &::aws_smithy_schema::Schema<'_> {
        Self::SCHEMA
    }

    #[allow(unused_variables, clippy::diverging_sub_expression)]
    fn serialize_members(
        &self,
        ser: &mut dyn ::aws_smithy_schema::serde::ShapeSerializer,
    ) -> ::std::result::Result<(), ::aws_smithy_schema::serde::SerdeError> {
        if let Some(ref val) = self.message {
            ser.write_string(&STREAMFAILURE_MEMBER_MESSAGE, val)?;
        }
        Ok(())
    }
}
impl StreamFailure {
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
    ) -> ::std::result::Result<crate::error::StreamFailure, ::aws_smithy_schema::serde::SerdeError> {
        let mut builder = crate::error::stream_failure::Builder::default();
        deserializer.read_struct(Self::SCHEMA, &mut |member, deser| {
            match member.member_index() {
                Some(0) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.message = Some(deser.read_string(member)?);
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        Ok(builder.build())
    }
}
impl StreamFailure {
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
        builder: &mut crate::error::stream_failure::Builder,
        deserializer: &mut dyn ::aws_smithy_schema::serde::ShapeDeserializer,
    ) -> ::std::result::Result<(), ::aws_smithy_schema::serde::SerdeError> {
        deserializer.read_struct(Self::SCHEMA, &mut |member, deser| {
            match member.member_index() {
                Some(0) => {
                    if deser.is_null() {
                        deser.read_null()?;
                    } else {
                        builder.message = Some(deser.read_string(member)?);
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        Ok(())
    }
}
impl StreamFailure {
    /// Returns the error message.
    pub fn message(&self) -> ::std::option::Option<&str> {
        self.message.as_deref()
    }
    #[doc(hidden)]
    /// Returns the error name.
    pub fn name(&self) -> &'static str {
        "StreamFailure"
    }
}
impl ::std::fmt::Display for StreamFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        ::std::write!(f, "StreamFailure")?;
        if let ::std::option::Option::Some(inner_1) = &self.message {
            {
                ::std::write!(f, ": {inner_1}")?;
            }
        }
        Ok(())
    }
}
impl ::std::error::Error for StreamFailure {}
impl ::aws_smithy_http_server::schema::HttpModeledError for StreamFailure {
    fn status_code(&self) -> u16 {
        500
    }
}
impl StreamFailure {
    /// Creates a new builder-style object to manufacture [`StreamFailure`](crate::error::StreamFailure).
    pub fn builder() -> crate::error::stream_failure::Builder {
        crate::error::stream_failure::Builder::default()
    }
}

/// Error type for the `Events` operation.
/// Each variant represents an error that can occur for the `Events` operation.
#[derive(::std::fmt::Debug)]
pub enum EventsError {
    #[allow(missing_docs)] // documentation missing in model
    StreamFailure(crate::error::StreamFailure),
}
impl ::std::fmt::Display for EventsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            EventsError::StreamFailure(_inner) => _inner.fmt(f),
        }
    }
}
impl EventsError {
    /// Returns `true` if the error kind is `EventsError::StreamFailure`.
    pub fn is_stream_failure(&self) -> bool {
        matches!(&self, EventsError::StreamFailure(_))
    }
    /// Returns the error name string by matching the correct variant.
    pub fn name(&self) -> &'static str {
        match &self {
            EventsError::StreamFailure(_inner) => _inner.name(),
        }
    }
}
impl ::std::error::Error for EventsError {
    fn source(&self) -> std::option::Option<&(dyn ::std::error::Error + 'static)> {
        match &self {
            EventsError::StreamFailure(_inner) => Some(_inner),
        }
    }
}
impl ::std::convert::From<crate::error::StreamFailure> for crate::error::EventsError {
    fn from(variant: crate::error::StreamFailure) -> crate::error::EventsError {
        Self::StreamFailure(variant)
    }
}
/// See [`StreamFailure`](crate::error::StreamFailure).
pub mod stream_failure {

    impl ::std::convert::From<Builder> for crate::error::StreamFailure {
        fn from(builder: Builder) -> Self {
            builder.build()
        }
    }
    /// A builder for [`StreamFailure`](crate::error::StreamFailure).
    #[derive(::std::clone::Clone, ::std::default::Default, ::std::fmt::Debug)]
    pub struct Builder {
        pub(crate) message: ::std::option::Option<::std::string::String>,
    }
    impl Builder {
        #[allow(missing_docs)] // documentation missing in model
        pub fn message(mut self, input: ::std::option::Option<::std::string::String>) -> Self {
            self.message = input;
            self
        }
        #[allow(missing_docs)] // documentation missing in model
        #[allow(dead_code)]
        pub(crate) fn set_message(mut self, input: Option<impl ::std::convert::Into<::std::string::String>>) -> Self {
            self.message = input.map(|v| v.into());
            self
        }
        /// Consumes the builder and constructs a [`StreamFailure`](crate::error::StreamFailure).
        pub fn build(self) -> crate::error::StreamFailure {
            self.build_enforcing_all_constraints()
        }
        fn build_enforcing_all_constraints(self) -> crate::error::StreamFailure {
            crate::error::StreamFailure { message: self.message }
        }
    }
}
