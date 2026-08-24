/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Frame-level event-stream binding interpretation (plan Step 4.8).
//!
//! The generated `Marshaller<P>` / `Unmarshaller<P>` types carry only model
//! facts (`:event-type` strings, which structure a variant maps to). How an
//! event structure's members map onto an event-stream frame — `@eventHeader`
//! members become frame headers, an `@eventPayload` member IS the payload,
//! everything else travels in one codec document — is interpreted HERE, off
//! the event structure's schema, exactly like the HTTP binding modules do for
//! requests and responses (principle 1: no protocol knowledge in generated
//! code).

use aws_smithy_eventstream::error::Error as EventStreamError;
use aws_smithy_schema::codec::{Codec, FinishSerializer};
use aws_smithy_schema::serde::{
    SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer,
};
use aws_smithy_schema::Schema;
use aws_smithy_types::event_stream::{Header, HeaderValue, Message};
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};

/// Marshals one event structure into an event-stream [`Message`].
///
/// Header layout mirrors the legacy generated marshallers: `:message-type`,
/// then the type header (`:event-type` / `:exception-type`), then
/// `@eventHeader` members in model order, then `:content-type` when a payload
/// is present.
///
/// Payload resolution, all off the schema:
/// - an `@eventPayload` blob/string member is the raw payload
///   (`application/octet-stream` / `text/plain`);
/// - an `@eventPayload` structure/union serializes standalone through
///   [`Codec`] (`payload_content_type`);
/// - otherwise, any non-`@eventHeader` members serialize as one codec
///   document (`payload_content_type`);
/// - no such members → empty payload, no `:content-type`.
pub fn marshall_event<C: Codec>(
    codec: &C,
    message_type: &'static str,
    type_header_name: &'static str,
    type_value: &str,
    payload_content_type: &'static str,
    schema: &Schema<'_>,
    event: &dyn SerializableStruct,
) -> Result<Message, EventStreamError> {
    let mut headers = vec![
        Header::new(
            ":message-type",
            HeaderValue::String(message_type.to_owned().into()),
        ),
        Header::new(
            type_header_name,
            HeaderValue::String(type_value.to_owned().into()),
        ),
    ];

    // Pass 1: capture `@eventHeader` members and any raw `@eventPayload`.
    let mut collector = EventHeaderCollector {
        headers: &mut headers,
        raw_payload: None,
        error: None,
    };
    event
        .serialize_members(&mut collector)
        .map_err(|err| EventStreamError::marshalling(format!("{err}")))?;
    if let Some(err) = collector.error {
        return Err(EventStreamError::marshalling(format!("{err}")));
    }
    let raw_payload = collector.raw_payload;

    let payload_member = schema.members().iter().any(|m| m.event_payload().is_some());
    let body_members = schema
        .members()
        .iter()
        .any(|m| m.event_header().is_none() && m.event_payload().is_none());

    let (payload, content_type): (Vec<u8>, Option<&'static str>) =
        if let Some((bytes, content_type)) = raw_payload {
            (bytes, Some(content_type))
        } else if payload_member {
            // A structure/union `@eventPayload`: serialize the payload member
            // standalone through the codec.
            let mut serializer = codec.create_serializer();
            let mut router = PayloadStructRouter {
                serializer: &mut serializer,
                served: false,
            };
            event
                .serialize_members(&mut router)
                .map_err(|err| EventStreamError::marshalling(format!("{err}")))?;
            (serializer.finish(), Some(payload_content_type))
        } else if body_members {
            let mut serializer = codec.create_serializer();
            serializer
                .write_struct(schema, &SkipBoundMembers(event))
                .map_err(|err| EventStreamError::marshalling(format!("{err}")))?;
            (serializer.finish(), Some(payload_content_type))
        } else {
            (Vec::new(), None)
        };

    if let Some(content_type) = content_type {
        headers.push(Header::new(
            ":content-type",
            HeaderValue::String(content_type.to_owned().into()),
        ));
    }
    Ok(Message::new_from_parts(headers, payload))
}

/// Builds an `initial-request` / `initial-response` message with the given
/// codec payload — the RPC-protocols-only initial-message framing.
pub fn initial_message(
    event_type: &'static str,
    payload_content_type: &'static str,
    payload: bytes::Bytes,
) -> Message {
    let headers = vec![
        Header::new(":message-type", HeaderValue::String("event".into())),
        Header::new(
            ":event-type",
            HeaderValue::String(event_type.to_owned().into()),
        ),
        Header::new(
            ":content-type",
            HeaderValue::String(payload_content_type.to_owned().into()),
        ),
    ];
    Message::new_from_parts(headers, payload)
}

macro_rules! forward_to_ignore {
    ($($fn_name:ident($ty:ty)),* $(,)?) => {
        $(fn $fn_name(&mut self, schema: &Schema<'_>, value: $ty) -> Result<(), SerdeError> {
            let _ = (schema, value);
            Ok(())
        })*
    };
}

/// Serializer proxy capturing `@eventHeader` members as frame [`Header`]s and
/// a raw `@eventPayload` blob/string; everything else is ignored on this pass
/// (the body pass picks it up).
struct EventHeaderCollector<'a> {
    headers: &'a mut Vec<Header>,
    raw_payload: Option<(Vec<u8>, &'static str)>,
    error: Option<SerdeError>,
}

impl EventHeaderCollector<'_> {
    fn push(&mut self, schema: &Schema<'_>, value: HeaderValue) {
        if let Some(name) = schema.member_name() {
            self.headers.push(Header::new(name.to_owned(), value));
        }
    }
}

impl ShapeSerializer for EventHeaderCollector<'_> {
    fn write_struct(
        &mut self,
        _schema: &Schema<'_>,
        _value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        Ok(())
    }

    fn write_list(
        &mut self,
        _schema: &Schema<'_>,
        _write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Ok(())
    }

    fn write_map(
        &mut self,
        _schema: &Schema<'_>,
        _write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Ok(())
    }

    fn write_boolean(&mut self, schema: &Schema<'_>, value: bool) -> Result<(), SerdeError> {
        if schema.event_header().is_some() {
            self.push(schema, HeaderValue::Bool(value));
        }
        Ok(())
    }

    fn write_byte(&mut self, schema: &Schema<'_>, value: i8) -> Result<(), SerdeError> {
        if schema.event_header().is_some() {
            self.push(schema, HeaderValue::Byte(value));
        }
        Ok(())
    }

    fn write_short(&mut self, schema: &Schema<'_>, value: i16) -> Result<(), SerdeError> {
        if schema.event_header().is_some() {
            self.push(schema, HeaderValue::Int16(value));
        }
        Ok(())
    }

    fn write_integer(&mut self, schema: &Schema<'_>, value: i32) -> Result<(), SerdeError> {
        if schema.event_header().is_some() {
            self.push(schema, HeaderValue::Int32(value));
        }
        Ok(())
    }

    fn write_long(&mut self, schema: &Schema<'_>, value: i64) -> Result<(), SerdeError> {
        if schema.event_header().is_some() {
            self.push(schema, HeaderValue::Int64(value));
        }
        Ok(())
    }

    fn write_string(&mut self, schema: &Schema<'_>, value: &str) -> Result<(), SerdeError> {
        if schema.event_header().is_some() {
            self.push(schema, HeaderValue::String(value.to_owned().into()));
        } else if schema.event_payload().is_some() {
            self.raw_payload = Some((value.as_bytes().to_vec(), "text/plain"));
        }
        Ok(())
    }

    fn write_blob(&mut self, schema: &Schema<'_>, value: &[u8]) -> Result<(), SerdeError> {
        if schema.event_header().is_some() {
            self.push(schema, HeaderValue::ByteArray(value.to_vec().into()));
        } else if schema.event_payload().is_some() {
            self.raw_payload = Some((value.to_vec(), "application/octet-stream"));
        }
        Ok(())
    }

    fn write_timestamp(&mut self, schema: &Schema<'_>, value: &DateTime) -> Result<(), SerdeError> {
        if schema.event_header().is_some() {
            self.push(schema, HeaderValue::Timestamp(*value));
        }
        Ok(())
    }

    forward_to_ignore! {
        write_float(f32),
        write_double(f64),
    }

    fn write_big_integer(
        &mut self,
        _schema: &Schema<'_>,
        _value: &BigInteger,
    ) -> Result<(), SerdeError> {
        Ok(())
    }

    fn write_big_decimal(
        &mut self,
        _schema: &Schema<'_>,
        _value: &BigDecimal,
    ) -> Result<(), SerdeError> {
        Ok(())
    }

    fn write_document(&mut self, _schema: &Schema<'_>, _value: &Document) -> Result<(), SerdeError> {
        Ok(())
    }

    fn write_null(&mut self, _schema: &Schema<'_>) -> Result<(), SerdeError> {
        Ok(())
    }
}

/// Serializer proxy routing ONLY the `@eventPayload` structure/union member
/// into the codec serializer (standalone, against the member schema).
struct PayloadStructRouter<'a, S> {
    serializer: &'a mut S,
    served: bool,
}

impl<S: ShapeSerializer> ShapeSerializer for PayloadStructRouter<'_, S> {
    fn write_struct(
        &mut self,
        schema: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        if schema.event_payload().is_some() && !self.served {
            self.served = true;
            return self.serializer.write_struct(schema, value);
        }
        Ok(())
    }

    fn write_list(
        &mut self,
        _schema: &Schema<'_>,
        _write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Ok(())
    }

    fn write_map(
        &mut self,
        _schema: &Schema<'_>,
        _write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Ok(())
    }

    forward_to_ignore! {
        write_boolean(bool),
        write_byte(i8),
        write_short(i16),
        write_integer(i32),
        write_long(i64),
        write_float(f32),
        write_double(f64),
    }

    fn write_string(&mut self, _schema: &Schema<'_>, _value: &str) -> Result<(), SerdeError> {
        Ok(())
    }

    fn write_blob(&mut self, _schema: &Schema<'_>, _value: &[u8]) -> Result<(), SerdeError> {
        Ok(())
    }

    fn write_timestamp(
        &mut self,
        _schema: &Schema<'_>,
        _value: &DateTime,
    ) -> Result<(), SerdeError> {
        Ok(())
    }

    fn write_big_integer(
        &mut self,
        _schema: &Schema<'_>,
        _value: &BigInteger,
    ) -> Result<(), SerdeError> {
        Ok(())
    }

    fn write_big_decimal(
        &mut self,
        _schema: &Schema<'_>,
        _value: &BigDecimal,
    ) -> Result<(), SerdeError> {
        Ok(())
    }

    fn write_document(&mut self, _schema: &Schema<'_>, _value: &Document) -> Result<(), SerdeError> {
        Ok(())
    }

    fn write_null(&mut self, _schema: &Schema<'_>) -> Result<(), SerdeError> {
        Ok(())
    }
}

/// A [`SerializableStruct`] view that drops `@eventHeader` / `@eventPayload`
/// members, so the codec body document contains only the unbound members.
struct SkipBoundMembers<'a>(&'a dyn SerializableStruct);

impl SerializableStruct for SkipBoundMembers<'_> {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        let mut router = DropBoundMembers { inner: serializer };
        self.0.serialize_members(&mut router)
    }
}

macro_rules! forward_unless_bound {
    ($($fn_name:ident($ty:ty)),* $(,)?) => {
        $(fn $fn_name(&mut self, schema: &Schema<'_>, value: $ty) -> Result<(), SerdeError> {
            if schema.event_header().is_some() || schema.event_payload().is_some() {
                return Ok(());
            }
            self.inner.$fn_name(schema, value)
        })*
    };
}

struct DropBoundMembers<'a> {
    inner: &'a mut dyn ShapeSerializer,
}

impl ShapeSerializer for DropBoundMembers<'_> {
    fn write_struct(
        &mut self,
        schema: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        if schema.event_header().is_some() || schema.event_payload().is_some() {
            return Ok(());
        }
        self.inner.write_struct(schema, value)
    }

    fn write_list(
        &mut self,
        schema: &Schema<'_>,
        write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        if schema.event_header().is_some() || schema.event_payload().is_some() {
            return Ok(());
        }
        self.inner.write_list(schema, write_elements)
    }

    fn write_map(
        &mut self,
        schema: &Schema<'_>,
        write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        if schema.event_header().is_some() || schema.event_payload().is_some() {
            return Ok(());
        }
        self.inner.write_map(schema, write_entries)
    }

    forward_unless_bound! {
        write_boolean(bool),
        write_byte(i8),
        write_short(i16),
        write_integer(i32),
        write_long(i64),
        write_float(f32),
        write_double(f64),
        write_string(&str),
        write_blob(&[u8]),
    }

    fn write_timestamp(&mut self, schema: &Schema<'_>, value: &DateTime) -> Result<(), SerdeError> {
        if schema.event_header().is_some() || schema.event_payload().is_some() {
            return Ok(());
        }
        self.inner.write_timestamp(schema, value)
    }

    fn write_big_integer(
        &mut self,
        schema: &Schema<'_>,
        value: &BigInteger,
    ) -> Result<(), SerdeError> {
        if schema.event_header().is_some() {
            return Ok(());
        }
        self.inner.write_big_integer(schema, value)
    }

    fn write_big_decimal(
        &mut self,
        schema: &Schema<'_>,
        value: &BigDecimal,
    ) -> Result<(), SerdeError> {
        if schema.event_header().is_some() {
            return Ok(());
        }
        self.inner.write_big_decimal(schema, value)
    }

    fn write_document(&mut self, schema: &Schema<'_>, value: &Document) -> Result<(), SerdeError> {
        if schema.event_header().is_some() {
            return Ok(());
        }
        self.inner.write_document(schema, value)
    }

    fn write_null(&mut self, schema: &Schema<'_>) -> Result<(), SerdeError> {
        self.inner.write_null(schema)
    }
}

// ============================================================================
// Unmarshalling
// ============================================================================

/// Composite deserializer over one event-stream frame: `@eventHeader` members
/// read from the frame headers, an `@eventPayload` member from the raw
/// payload, everything else from the payload as one codec document. Drives the
/// generated schema walker of the event structure — the frame counterpart of
/// the HTTP request composite.
pub struct EventFrameDeserializer<'a, C> {
    codec: &'a C,
    message: &'a Message,
}

impl<'a, C: Codec> EventFrameDeserializer<'a, C> {
    /// Creates a deserializer reading [`message`] through [`codec`].
    pub fn new(codec: &'a C, message: &'a Message) -> Self {
        Self { codec, message }
    }
}

impl<C: Codec> ShapeDeserializer for EventFrameDeserializer<'_, C> {
    fn read_struct(
        &mut self,
        schema: &Schema<'_>,
        consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let mut has_body_members = false;
        for member in schema.members() {
            if member.event_header().is_some() {
                let Some(name) = member.member_name() else {
                    continue;
                };
                if let Some(header) = self
                    .message
                    .headers()
                    .iter()
                    .find(|h| h.name().as_str() == name)
                {
                    let mut deser = HeaderValueDeserializer {
                        value: header.value(),
                    };
                    consumer(member, &mut deser)?;
                }
            } else if member.event_payload().is_some() {
                match member.shape_type() {
                    aws_smithy_schema::ShapeType::Blob | aws_smithy_schema::ShapeType::String => {
                        let mut deser = RawPayloadDeserializer {
                            payload: self.message.payload(),
                        };
                        consumer(member, &mut deser)?;
                    }
                    _ => {
                        let mut deser = self.codec.create_deserializer(self.message.payload());
                        consumer(member, &mut deser)?;
                    }
                }
            } else {
                has_body_members = true;
            }
        }
        if has_body_members && !self.message.payload().is_empty() {
            let mut body = self.codec.create_deserializer(self.message.payload());
            body.read_struct(schema, consumer)?;
        }
        Ok(())
    }

    fn read_list(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported("an event is a structure"))
    }

    fn read_map(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported("an event is a structure"))
    }

    fn read_boolean(&mut self, _: &Schema<'_>) -> Result<bool, SerdeError> {
        Err(SerdeError::unsupported("an event is a structure"))
    }

    fn read_byte(&mut self, _: &Schema<'_>) -> Result<i8, SerdeError> {
        Err(SerdeError::unsupported("an event is a structure"))
    }

    fn read_short(&mut self, _: &Schema<'_>) -> Result<i16, SerdeError> {
        Err(SerdeError::unsupported("an event is a structure"))
    }

    fn read_integer(&mut self, _: &Schema<'_>) -> Result<i32, SerdeError> {
        Err(SerdeError::unsupported("an event is a structure"))
    }

    fn read_long(&mut self, _: &Schema<'_>) -> Result<i64, SerdeError> {
        Err(SerdeError::unsupported("an event is a structure"))
    }

    fn read_float(&mut self, _: &Schema<'_>) -> Result<f32, SerdeError> {
        Err(SerdeError::unsupported("an event is a structure"))
    }

    fn read_double(&mut self, _: &Schema<'_>) -> Result<f64, SerdeError> {
        Err(SerdeError::unsupported("an event is a structure"))
    }

    fn read_big_integer(&mut self, _: &Schema<'_>) -> Result<BigInteger, SerdeError> {
        Err(SerdeError::unsupported("an event is a structure"))
    }

    fn read_big_decimal(&mut self, _: &Schema<'_>) -> Result<BigDecimal, SerdeError> {
        Err(SerdeError::unsupported("an event is a structure"))
    }

    fn read_string(&mut self, _: &Schema<'_>) -> Result<String, SerdeError> {
        Err(SerdeError::unsupported("an event is a structure"))
    }

    fn read_blob(&mut self, _: &Schema<'_>) -> Result<Blob, SerdeError> {
        Err(SerdeError::unsupported("an event is a structure"))
    }

    fn read_timestamp(&mut self, _: &Schema<'_>) -> Result<DateTime, SerdeError> {
        Err(SerdeError::unsupported("an event is a structure"))
    }

    fn read_document(&mut self, _: &Schema<'_>) -> Result<Document, SerdeError> {
        Err(SerdeError::unsupported("an event is a structure"))
    }

    fn is_null(&self) -> bool {
        false
    }

    fn container_size(&self) -> Option<usize> {
        None
    }
}

macro_rules! header_type_mismatch {
    ($($fn_name:ident -> $ty:ty),* $(,)?) => {
        $(fn $fn_name(&mut self, _: &Schema<'_>) -> Result<$ty, SerdeError> {
            Err(SerdeError::type_mismatch("unsupported event header value type"))
        })*
    };
}

/// Reads one frame-header value as the member's scalar type.
struct HeaderValueDeserializer<'a> {
    value: &'a HeaderValue,
}

impl ShapeDeserializer for HeaderValueDeserializer<'_> {
    fn read_struct(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::type_mismatch("event headers are scalar"))
    }

    fn read_list(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::type_mismatch("event headers are scalar"))
    }

    fn read_map(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::type_mismatch("event headers are scalar"))
    }

    fn read_boolean(&mut self, _: &Schema<'_>) -> Result<bool, SerdeError> {
        match self.value {
            HeaderValue::Bool(value) => Ok(*value),
            _ => Err(SerdeError::type_mismatch("expected bool event header")),
        }
    }

    fn read_byte(&mut self, _: &Schema<'_>) -> Result<i8, SerdeError> {
        match self.value {
            HeaderValue::Byte(value) => Ok(*value),
            _ => Err(SerdeError::type_mismatch("expected byte event header")),
        }
    }

    fn read_short(&mut self, _: &Schema<'_>) -> Result<i16, SerdeError> {
        match self.value {
            HeaderValue::Int16(value) => Ok(*value),
            _ => Err(SerdeError::type_mismatch("expected int16 event header")),
        }
    }

    fn read_integer(&mut self, _: &Schema<'_>) -> Result<i32, SerdeError> {
        match self.value {
            HeaderValue::Int32(value) => Ok(*value),
            _ => Err(SerdeError::type_mismatch("expected int32 event header")),
        }
    }

    fn read_long(&mut self, _: &Schema<'_>) -> Result<i64, SerdeError> {
        match self.value {
            HeaderValue::Int64(value) => Ok(*value),
            _ => Err(SerdeError::type_mismatch("expected int64 event header")),
        }
    }

    fn read_string(&mut self, _: &Schema<'_>) -> Result<String, SerdeError> {
        match self.value {
            HeaderValue::String(value) => Ok(value.as_str().to_owned()),
            _ => Err(SerdeError::type_mismatch("expected string event header")),
        }
    }

    fn read_blob(&mut self, _: &Schema<'_>) -> Result<Blob, SerdeError> {
        match self.value {
            HeaderValue::ByteArray(value) => Ok(Blob::new(value.to_vec())),
            _ => Err(SerdeError::type_mismatch("expected byte-array event header")),
        }
    }

    fn read_timestamp(&mut self, _: &Schema<'_>) -> Result<DateTime, SerdeError> {
        match self.value {
            HeaderValue::Timestamp(value) => Ok(*value),
            _ => Err(SerdeError::type_mismatch("expected timestamp event header")),
        }
    }

    header_type_mismatch! {
        read_float -> f32,
        read_double -> f64,
        read_big_integer -> BigInteger,
        read_big_decimal -> BigDecimal,
        read_document -> Document,
    }

    fn is_null(&self) -> bool {
        false
    }

    fn container_size(&self) -> Option<usize> {
        None
    }
}

/// Reads the raw frame payload as a blob or a UTF-8 string.
struct RawPayloadDeserializer<'a> {
    payload: &'a [u8],
}

impl ShapeDeserializer for RawPayloadDeserializer<'_> {
    fn read_struct(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::type_mismatch("raw payloads are blobs or strings"))
    }

    fn read_list(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::type_mismatch("raw payloads are blobs or strings"))
    }

    fn read_map(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::type_mismatch("raw payloads are blobs or strings"))
    }

    fn read_string(&mut self, _: &Schema<'_>) -> Result<String, SerdeError> {
        String::from_utf8(self.payload.to_vec())
            .map_err(|_| SerdeError::invalid_input("event payload is not valid UTF-8"))
    }

    fn read_blob(&mut self, _: &Schema<'_>) -> Result<Blob, SerdeError> {
        Ok(Blob::new(self.payload.to_vec()))
    }

    header_type_mismatch! {
        read_boolean -> bool,
        read_byte -> i8,
        read_short -> i16,
        read_integer -> i32,
        read_long -> i64,
        read_float -> f32,
        read_double -> f64,
        read_big_integer -> BigInteger,
        read_big_decimal -> BigDecimal,
        read_timestamp -> DateTime,
        read_document -> Document,
    }

    fn is_null(&self) -> bool {
        false
    }

    fn container_size(&self) -> Option<usize> {
        None
    }
}
