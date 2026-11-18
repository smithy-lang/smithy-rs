/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! One-time snapshot of generated event-stream benchmark SDK code. See README.md.
#![allow(dead_code)]

/// Unmarshals `Events` frames through the runtime-selected protocol.
pub type EventsUnmarshaller =
    ::aws_smithy_http_server::schema::event_stream::SchemaEventUnmarshaller<crate::model::Events>;

impl ::aws_smithy_http_server::schema::event_stream::DeserializableEventStream for crate::model::Events {
    type Error = crate::error::EventsError;

    fn deserialize_event(
        event_type: &str,
        frame: &::aws_smithy_http_server::schema::event_stream::EventFrame<'_>,
    ) -> ::std::result::Result<::std::option::Option<Self>, ::aws_smithy_eventstream::error::Error> {
        match event_type {
            "structured" => {
                let mut deser = frame.deserializer();
                let parsed = crate::model::StructuredEvent::deserialize_schema(&mut deser).map_err(|err| {
                    ::aws_smithy_eventstream::error::Error::unmarshalling(format!(
                        "failed to unmarshall structured: {err}"
                    ))
                })?;

                ::std::result::Result::Ok(::std::option::Option::Some(crate::model::Events::Structured(parsed)))
            }
            "binary" => {
                let mut deser = frame.deserializer();
                let parsed = crate::model::BinaryEvent::deserialize_schema(&mut deser).map_err(|err| {
                    ::aws_smithy_eventstream::error::Error::unmarshalling(format!("failed to unmarshall binary: {err}"))
                })?;

                ::std::result::Result::Ok(::std::option::Option::Some(crate::model::Events::Binary(parsed)))
            }
            _ => ::std::result::Result::Ok(::std::option::Option::None),
        }
    }

    fn deserialize_error(
        exception_type: &str,
        frame: &::aws_smithy_http_server::schema::event_stream::EventFrame<'_>,
    ) -> ::std::result::Result<::std::option::Option<Self::Error>, ::aws_smithy_eventstream::error::Error> {
        match exception_type {
            "failure" => {
                let mut deser = frame.deserializer();
                let parsed = crate::error::StreamFailure::deserialize_schema(&mut deser).map_err(|err| {
                    ::aws_smithy_eventstream::error::Error::unmarshalling(format!(
                        "failed to unmarshall exception: {err}"
                    ))
                })?;

                ::std::result::Result::Ok(::std::option::Option::Some(crate::error::EventsError::StreamFailure(
                    parsed,
                )))
            }
            _ => ::std::result::Result::Ok(::std::option::Option::None),
        }
    }
}

/// Marshals `Events` events into frames through the runtime-selected protocol.
pub type EventsMarshaller = ::aws_smithy_http_server::schema::event_stream::SchemaEventMarshaller<crate::model::Events>;

/// Marshals `Events` errors into `exception` frames through the runtime-selected protocol.
pub type EventsErrorMarshaller =
    ::aws_smithy_http_server::schema::event_stream::SchemaEventErrorMarshaller<crate::error::EventsError>;

impl ::aws_smithy_http_server::schema::event_stream::SerializableEventError for crate::error::EventsError {
    fn variant(
        &self,
    ) -> (
        &'static str,
        &::aws_smithy_schema::Schema<'_>,
        &dyn ::aws_smithy_schema::serde::SerializableStruct,
    ) {
        match self {
            Self::StreamFailure(inner) => ("failure", crate::error::StreamFailure::SCHEMA, inner),
        }
    }
}
