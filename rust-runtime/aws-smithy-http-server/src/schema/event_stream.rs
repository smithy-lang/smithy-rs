/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Schema-driven event stream marshalling and unmarshalling.
//!
//! Frames are built and read against the shapes' runtime [`Schema`]s instead of generated
//! per-shape marshallers: member schemas carry the `@eventHeader` and `@eventPayload` traits,
//! and the payload travels through the codec of whichever protocol routing selected. Generated
//! code contributes only the pieces that need shape types — a [`DeserializableEventStream`]
//! dispatch per stream union and a [`SerializableEventError`] dispatch per error enum — and
//! aliases its `*Marshaller`/`*Unmarshaller` types to the generics here.
//!
//! The event framing follows the AWS event stream contract:
//! - a data event carries `:message-type: event` and `:event-type` (the Smithy member name),
//! - an in-band error carries `:message-type: exception` and `:exception-type`,
//! - `@eventHeader` members become message headers, an `@eventPayload` member becomes the
//!   payload (raw bytes for blobs, UTF-8 for strings, codec-encoded otherwise). Without an
//!   explicit payload member, non-header members form a codec-encoded payload document,
//! - `:content-type` labels the payload: `application/octet-stream` and `text/plain` are fixed
//!   by the member's shape, everything else is the selected protocol's event stream media type.

use std::fmt;
use std::marker::PhantomData;

use aws_smithy_eventstream::error::Error;
use aws_smithy_eventstream::frame::{
    MarshallMessage, NoOpSigner, UnmarshallMessage, UnmarshalledMessage,
};
use aws_smithy_eventstream::smithy as expect_fns;
use aws_smithy_http::event_stream::{
    EventOrInitial, EventOrInitialMarshaller, EventStreamSender, InitialMessageType,
};
use aws_smithy_schema::serde::{
    SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer,
};
use aws_smithy_schema::{Schema, ShapeType};
use aws_smithy_types::event_stream::{Header, HeaderValue, Message};
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};
use bytes::Bytes;

use crate::body::{boxed, BoxBody};
use crate::schema::{DeserializeError, EventStreamFraming, SharedServerProtocol};

const NO_EVENT_STREAM_SUPPORT: &str = "protocol does not support event streams";

fn capability_or_marshalling_error(
    protocol: &SharedServerProtocol,
) -> Result<EventStreamFraming<'_>, Error> {
    protocol
        .event_stream_framing()
        .ok_or_else(|| Error::marshalling(NO_EVENT_STREAM_SUPPORT.to_owned()))
}

// ---------------------------------------------------------------------------
// Marshalling
// ---------------------------------------------------------------------------

/// Marshals the events of a schema-mode event stream union into frames.
///
/// `T` is the generated stream union; its [`SerializableStruct::serialize_members`] names the
/// active variant, and the variant's member schemas drive header and payload placement.
pub struct SchemaEventMarshaller<T> {
    protocol: SharedServerProtocol,
    _phantom: PhantomData<fn() -> T>,
}

impl<T> SchemaEventMarshaller<T> {
    /// Creates a marshaller that frames events through `protocol`'s event stream capability.
    pub fn new(protocol: SharedServerProtocol) -> Self {
        Self {
            protocol,
            _phantom: PhantomData,
        }
    }
}

impl<T> fmt::Debug for SchemaEventMarshaller<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchemaEventMarshaller")
            .field("protocol", &self.protocol)
            .finish()
    }
}

impl<T: SerializableStruct> MarshallMessage for SchemaEventMarshaller<T> {
    type Input = T;

    fn marshall(&self, input: Self::Input) -> Result<Message, Error> {
        let capability = capability_or_marshalling_error(&self.protocol)?;
        let mut router = UnionVariantSerializer {
            capability,
            kind: FrameKind::Event,
            message: None,
        };
        input
            .serialize_members(&mut router)
            .map_err(|err| Error::marshalling(format!("{err}")))?;
        router
            .message
            .ok_or_else(|| Error::marshalling("event stream union serialized no variant".to_owned()))
    }
}

/// Names the active variant of a generated event stream error enum.
///
/// Implemented by generated code; the returned name is the original Smithy member name of the
/// error in the stream union, which becomes the frame's `:exception-type`.
pub trait SerializableEventError {
    /// Returns the `:exception-type` value and the modeled error to frame.
    fn variant(&self) -> (&'static str, &dyn SerializableStruct);
}

/// Marshals the modeled errors of a schema-mode event stream union into `exception` frames.
pub struct SchemaEventErrorMarshaller<E> {
    protocol: SharedServerProtocol,
    _phantom: PhantomData<fn() -> E>,
}

impl<E> SchemaEventErrorMarshaller<E> {
    /// Creates an error marshaller that frames errors through `protocol`'s event stream capability.
    pub fn new(protocol: SharedServerProtocol) -> Self {
        Self {
            protocol,
            _phantom: PhantomData,
        }
    }
}

impl<E> fmt::Debug for SchemaEventErrorMarshaller<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchemaEventErrorMarshaller")
            .field("protocol", &self.protocol)
            .finish()
    }
}

impl<E: SerializableEventError> MarshallMessage for SchemaEventErrorMarshaller<E> {
    type Input = E;

    fn marshall(&self, input: Self::Input) -> Result<Message, Error> {
        let capability = capability_or_marshalling_error(&self.protocol)?;
        let (exception_type, value) = input.variant();
        build_frame(capability, exception_type, value, FrameKind::Exception)
            .map_err(|err| Error::marshalling(format!("{err}")))
    }
}

/// The error marshaller of a stream union with no modeled `@error` members.
///
/// Only [`aws_smithy_http::event_stream::MessageStreamError`] can reach it, and that error is
/// not modeled on the wire: the frame carries `:message-type: exception` and nothing else.
#[derive(Debug)]
pub struct NoModeledEventErrorMarshaller {
    protocol: SharedServerProtocol,
}

impl NoModeledEventErrorMarshaller {
    /// Creates the marshaller. The protocol is only used to reject protocols without event
    /// stream support, keeping the constructor uniform with the modeled marshallers.
    pub fn new(protocol: SharedServerProtocol) -> Self {
        Self { protocol }
    }
}

impl MarshallMessage for NoModeledEventErrorMarshaller {
    type Input = aws_smithy_http::event_stream::MessageStreamError;

    fn marshall(&self, _input: Self::Input) -> Result<Message, Error> {
        capability_or_marshalling_error(&self.protocol)?;
        let headers = vec![Header::new(
            ":message-type",
            HeaderValue::String("exception".into()),
        )];
        Ok(Message::new_from_parts(headers, Bytes::new()))
    }
}

#[derive(Clone, Copy)]
enum FrameKind {
    Event,
    Exception,
}

/// Builds one event or exception frame from a serializable event struct.
fn build_frame(
    capability: EventStreamFraming<'_>,
    event_type: &str,
    value: &dyn SerializableStruct,
    kind: FrameKind,
) -> Result<Message, SerdeError> {
    let (message_type, type_header) = match kind {
        FrameKind::Event => ("event", ":event-type"),
        FrameKind::Exception => ("exception", ":exception-type"),
    };
    let mut headers = vec![
        Header::new(":message-type", HeaderValue::String(message_type.into())),
        Header::new(type_header, HeaderValue::String(event_type.to_string().into())),
    ];
    let schema = value.schema();
    let payload_member = schema.members().iter().copied().find(|m| m.event_payload());
    let has_header_members = schema.members().iter().any(|m| m.event_header());

    if payload_member.is_none() && !has_header_members {
        // No event traits at all (including empty structs): the whole struct is the payload,
        // encoded by the selected protocol's codec.
        headers.push(Header::new(
            ":content-type",
            HeaderValue::String(capability.media_type.to_string().into()),
        ));
        let mut ser = capability.payload_codec.create_serializer();
        ser.write_struct(schema, value)?;
        let payload = Bytes::from(ser.finish_boxed());
        return Ok(Message::new_from_parts(headers, payload));
    }

    let mut payload = None;
    {
        let mut member_ser = EventMemberSerializer {
            capability,
            headers: &mut headers,
            payload: &mut payload,
        };
        value.serialize_members(&mut member_ser)?;
    }
    match payload_member {
        Some(member) => {
            // `:content-type` is decided by the payload member's shape, not by whether the
            // optional value was present, matching the generated marshallers this replaces.
            let content_type = match member.shape_type() {
                ShapeType::Blob => "application/octet-stream",
                ShapeType::String => "text/plain",
                _ => capability.media_type,
            };
            headers.push(Header::new(
                ":content-type",
                HeaderValue::String(content_type.to_string().into()),
            ));
            Ok(Message::new_from_parts(headers, payload.unwrap_or_default()))
        }
        None if schema.members().iter().any(|m| !m.event_header()) => {
            // Headers are carried separately; the remaining members form one protocol
            // document. Keep the original schema so codecs retain structure-level traits.
            let mut ser = capability.payload_codec.create_serializer();
            ser.write_struct(schema, &ImplicitEventPayload(value))?;
            headers.push(Header::new(
                ":content-type",
                HeaderValue::String(capability.media_type.to_string().into()),
            ));
            Ok(Message::new_from_parts(headers, Bytes::from(ser.finish_boxed())))
        }
        // Header-only events: empty payload and no `:content-type`.
        None => Ok(Message::new_from_parts(headers, Bytes::new())),
    }
}

/// Presents the ordinary members as a payload document while retaining the event schema's
/// codec traits. Filtering applies only to the event's top-level members, not nested values.
struct ImplicitEventPayload<'a>(&'a dyn SerializableStruct);

impl SerializableStruct for ImplicitEventPayload<'_> {
    fn schema(&self) -> &Schema<'_> {
        self.0.schema()
    }

    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        self.0.serialize_members(&mut NonHeaderSerializer(serializer))
    }
}

struct NonHeaderSerializer<'a>(&'a mut dyn ShapeSerializer);

macro_rules! forward_non_header_members {
    ($($method:ident($($argument:ident: $ty:ty),*);)*) => {
        $(fn $method(&mut self, schema: &Schema<'_>, $($argument: $ty),*) -> Result<(), SerdeError> {
            if schema.event_header() {
                Ok(())
            } else {
                self.0.$method(schema, $($argument),*)
            }
        })*
    };
}

impl ShapeSerializer for NonHeaderSerializer<'_> {
    forward_non_header_members! {
        write_struct(value: &dyn SerializableStruct);
        write_list(write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>);
        write_map(write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>);
        write_boolean(value: bool);
        write_byte(value: i8);
        write_short(value: i16);
        write_integer(value: i32);
        write_long(value: i64);
        write_float(value: f32);
        write_double(value: f64);
        write_big_integer(value: &BigInteger);
        write_big_decimal(value: &BigDecimal);
        write_string(value: &str);
        write_blob(value: Blob);
        write_timestamp(value: &DateTime);
        write_document(value: &Document);
        write_null();
    }
}

/// Receives the single `write_struct` a stream union's `serialize_members` makes for its active
/// variant and builds the frame from it.
struct UnionVariantSerializer<'a> {
    capability: EventStreamFraming<'a>,
    kind: FrameKind,
    message: Option<Message>,
}

macro_rules! not_a_union_variant {
    ($($fn_name:ident($value_type:ty),)+) => {
        $(fn $fn_name(&mut self, _schema: &Schema<'_>, _value: $value_type) -> Result<(), SerdeError> {
            Err(SerdeError::unsupported(
                "an event stream union serializes exactly one structure variant",
            ))
        })+
    };
}

impl ShapeSerializer for UnionVariantSerializer<'_> {
    fn write_struct(
        &mut self,
        schema: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        let event_type = schema.member_name().ok_or_else(|| {
            SerdeError::unsupported("event stream union variant schema has no member name")
        })?;
        self.message = Some(build_frame(
            self.capability,
            event_type,
            value,
            self.kind,
        )?);
        Ok(())
    }

    fn write_list(
        &mut self,
        _schema: &Schema<'_>,
        _write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "an event stream union serializes exactly one structure variant",
        ))
    }

    fn write_map(
        &mut self,
        _schema: &Schema<'_>,
        _write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "an event stream union serializes exactly one structure variant",
        ))
    }

    not_a_union_variant!(
        write_boolean(bool),
        write_byte(i8),
        write_short(i16),
        write_integer(i32),
        write_long(i64),
        write_float(f32),
        write_double(f64),
        write_big_integer(&BigInteger),
        write_big_decimal(&BigDecimal),
        write_string(&str),
        write_blob(Blob),
        write_timestamp(&DateTime),
        write_document(&Document),
    );

    fn write_null(&mut self, _schema: &Schema<'_>) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "an event stream union serializes exactly one structure variant",
        ))
    }
}

/// Routes the members of one event struct into the frame being built.
///
/// `@eventHeader` members become message headers in write order; the `@eventPayload` member
/// becomes the payload. Ordinary members are handled separately by [`ImplicitEventPayload`].
struct EventMemberSerializer<'a> {
    capability: EventStreamFraming<'a>,
    headers: &'a mut Vec<Header>,
    payload: &'a mut Option<Bytes>,
}

enum MemberRole {
    Header,
    Payload,
    Dropped,
}

fn member_role(schema: &Schema<'_>) -> MemberRole {
    if schema.event_header() {
        MemberRole::Header
    } else if schema.event_payload() {
        MemberRole::Payload
    } else {
        MemberRole::Dropped
    }
}

impl EventMemberSerializer<'_> {
    fn push_header(&mut self, schema: &Schema<'_>, value: HeaderValue) -> Result<(), SerdeError> {
        let name = schema
            .member_name()
            .ok_or_else(|| SerdeError::unsupported("event header schema has no member name"))?;
        self.headers.push(Header::new(name.to_owned(), value));
        Ok(())
    }

    fn unsupported(&self, what: &str, schema: &Schema<'_>) -> SerdeError {
        SerdeError::unsupported(format!(
            "unsupported event stream {what} type: {:?}",
            schema.shape_type()
        ))
    }
}

macro_rules! header_only_member {
    ($($fn_name:ident($value_type:ty) => $header_value:expr,)+) => {
        $(fn $fn_name(&mut self, schema: &Schema<'_>, value: $value_type) -> Result<(), SerdeError> {
            match member_role(schema) {
                MemberRole::Header => {
                    #[allow(clippy::redundant_closure_call)]
                    self.push_header(schema, ($header_value)(value))
                }
                MemberRole::Payload => Err(self.unsupported("payload", schema)),
                MemberRole::Dropped => Ok(()),
            }
        })+
    };
}

macro_rules! dropped_only_member {
    ($($fn_name:ident($value_type:ty),)+) => {
        $(fn $fn_name(&mut self, schema: &Schema<'_>, _value: $value_type) -> Result<(), SerdeError> {
            match member_role(schema) {
                MemberRole::Header => Err(self.unsupported("header", schema)),
                MemberRole::Payload => Err(self.unsupported("payload", schema)),
                MemberRole::Dropped => Ok(()),
            }
        })+
    };
}

impl ShapeSerializer for EventMemberSerializer<'_> {
    fn write_struct(
        &mut self,
        schema: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        match member_role(schema) {
            MemberRole::Header => Err(self.unsupported("header", schema)),
            MemberRole::Payload => {
                let mut ser = self.capability.payload_codec.create_serializer();
                ser.write_struct(value.schema(), value)?;
                *self.payload = Some(Bytes::from(ser.finish_boxed()));
                Ok(())
            }
            MemberRole::Dropped => Ok(()),
        }
    }

    fn write_list(
        &mut self,
        schema: &Schema<'_>,
        _write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        match member_role(schema) {
            MemberRole::Header => Err(self.unsupported("header", schema)),
            MemberRole::Payload => Err(self.unsupported("payload", schema)),
            MemberRole::Dropped => Ok(()),
        }
    }

    fn write_map(
        &mut self,
        schema: &Schema<'_>,
        _write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        match member_role(schema) {
            MemberRole::Header => Err(self.unsupported("header", schema)),
            MemberRole::Payload => Err(self.unsupported("payload", schema)),
            MemberRole::Dropped => Ok(()),
        }
    }

    fn write_string(&mut self, schema: &Schema<'_>, value: &str) -> Result<(), SerdeError> {
        match member_role(schema) {
            MemberRole::Header => {
                self.push_header(schema, HeaderValue::String(value.to_owned().into()))
            }
            MemberRole::Payload => {
                *self.payload = Some(Bytes::from(value.to_owned().into_bytes()));
                Ok(())
            }
            MemberRole::Dropped => Ok(()),
        }
    }

    fn write_blob(&mut self, schema: &Schema<'_>, value: Blob) -> Result<(), SerdeError> {
        match member_role(schema) {
            MemberRole::Header => {
                self.push_header(schema, HeaderValue::ByteArray(value.into_bytes()))
            }
            MemberRole::Payload => {
                *self.payload = Some(value.into_bytes());
                Ok(())
            }
            MemberRole::Dropped => Ok(()),
        }
    }

    // Event stream header types: https://smithy.io/2.0/spec/streaming.html#eventheader-trait
    // Note: there are no floating point header types for Event Stream.
    header_only_member!(
        write_boolean(bool) => HeaderValue::Bool,
        write_byte(i8) => HeaderValue::Byte,
        write_short(i16) => HeaderValue::Int16,
        write_integer(i32) => HeaderValue::Int32,
        write_long(i64) => HeaderValue::Int64,
    );

    fn write_timestamp(&mut self, schema: &Schema<'_>, value: &DateTime) -> Result<(), SerdeError> {
        match member_role(schema) {
            MemberRole::Header => self.push_header(schema, HeaderValue::Timestamp(*value)),
            MemberRole::Payload => Err(self.unsupported("payload", schema)),
            MemberRole::Dropped => Ok(()),
        }
    }

    dropped_only_member!(
        write_float(f32),
        write_double(f64),
        write_big_integer(&BigInteger),
        write_big_decimal(&BigDecimal),
        write_document(&Document),
    );

    fn write_null(&mut self, _schema: &Schema<'_>) -> Result<(), SerdeError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Unmarshalling
// ---------------------------------------------------------------------------

/// A schema-mode event stream union that can be read back from frames.
///
/// Implemented by generated code: both methods dispatch on the member name and hand
/// [`EventFrame::deserializer`] to the event struct's schema-guided walker, returning
/// `Ok(None)` for names the union does not model.
pub trait DeserializableEventStream: Sized {
    /// The generated error type of the stream union, or
    /// [`MessageStreamError`](aws_smithy_http::event_stream::MessageStreamError) when the union
    /// models no errors.
    type Error;

    /// Reads the event named by the frame's `:event-type`.
    fn deserialize_event(event_type: &str, frame: &EventFrame<'_>)
        -> Result<Option<Self>, Error>;

    /// Reads the error named by the frame's `:exception-type`.
    fn deserialize_error(
        exception_type: &str,
        frame: &EventFrame<'_>,
    ) -> Result<Option<Self::Error>, Error>;
}

/// Unmarshals schema-mode event stream frames into a generated stream union.
pub struct SchemaEventUnmarshaller<T> {
    protocol: SharedServerProtocol,
    _phantom: PhantomData<fn() -> T>,
}

impl<T> SchemaEventUnmarshaller<T> {
    /// Creates an unmarshaller that reads payloads through `protocol`'s event stream capability.
    pub fn new(protocol: SharedServerProtocol) -> Self {
        Self {
            protocol,
            _phantom: PhantomData,
        }
    }
}

impl<T> fmt::Debug for SchemaEventUnmarshaller<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchemaEventUnmarshaller")
            .field("protocol", &self.protocol)
            .finish()
    }
}

impl<T: DeserializableEventStream> UnmarshallMessage for SchemaEventUnmarshaller<T> {
    type Output = T;
    type Error = T::Error;

    fn unmarshall(
        &self,
        message: &Message,
    ) -> Result<UnmarshalledMessage<Self::Output, Self::Error>, Error> {
        let capability = self
            .protocol
            .event_stream_framing()
            .ok_or_else(|| Error::unmarshalling(NO_EVENT_STREAM_SUPPORT))?;
        let response_headers = expect_fns::parse_response_headers(message)?;
        let frame = EventFrame {
            message,
            capability,
        };
        match response_headers.message_type.as_str() {
            "event" => {
                let event_type = response_headers.smithy_type.as_str();
                match T::deserialize_event(event_type, &frame)? {
                    Some(event) => Ok(UnmarshalledMessage::Event(event)),
                    None => Err(Error::unmarshalling(format!(
                        "unrecognized :event-type: {event_type}"
                    ))),
                }
            }
            "exception" => {
                let exception_type = response_headers.smithy_type.as_str();
                match T::deserialize_error(exception_type, &frame)? {
                    Some(error) => Ok(UnmarshalledMessage::Error(error)),
                    None => Err(Error::unmarshalling(format!(
                        "unrecognized exception: {exception_type}"
                    ))),
                }
            }
            value => Err(Error::unmarshalling(format!(
                "unrecognized :message-type: {value}"
            ))),
        }
    }
}

/// One received event stream frame together with the protocol capability that decodes it.
pub struct EventFrame<'a> {
    message: &'a Message,
    capability: EventStreamFraming<'a>,
}

impl<'a> EventFrame<'a> {
    /// Creates a frame view over `message`.
    pub fn new(message: &'a Message, capability: EventStreamFraming<'a>) -> Self {
        Self {
            message,
            capability,
        }
    }

    /// Returns a [`ShapeDeserializer`] that reads an event struct from this frame, routing
    /// `@eventHeader` members from the message headers and the payload through the codec.
    pub fn deserializer(&self) -> EventFrameDeserializer<'a> {
        EventFrameDeserializer {
            message: self.message,
            capability: self.capability,
        }
    }
}

/// Reads an event struct from a frame, guided by the struct's schema.
///
/// Only [`ShapeDeserializer::read_struct`] is meaningful at the top level; the member values it
/// hands to the consumer come from the message headers or from a codec deserializer over the
/// payload.
pub struct EventFrameDeserializer<'a> {
    message: &'a Message,
    capability: EventStreamFraming<'a>,
}

impl EventFrameDeserializer<'_> {
    fn content_type(&self) -> Option<&str> {
        self.message
            .headers()
            .iter()
            .find(|header| header.name().as_str() == ":content-type")
            .and_then(|header| header.value().as_string().ok())
            .map(|value| value.as_str())
    }

    fn check_content_type(&self, expected: &str) -> Result<(), SerdeError> {
        let content_type = self.content_type().unwrap_or_default();
        if content_type != expected {
            return Err(SerdeError::custom(format!(
                "expected :content-type to be '{expected}', but was '{content_type}'"
            )));
        }
        Ok(())
    }
}

macro_rules! frames_deserialize_structs {
    ($($fn_name:ident() -> $result_type:ty,)+) => {
        $(fn $fn_name(&mut self, _schema: &Schema<'_>) -> Result<$result_type, SerdeError> {
            Err(SerdeError::unsupported(
                "an event stream frame deserializes a structure",
            ))
        })+
    };
}

impl ShapeDeserializer for EventFrameDeserializer<'_> {
    fn read_struct(
        &mut self,
        schema: &Schema<'_>,
        state: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let members = schema.members();
        // Don't attempt to parse the payload for an empty struct. The payload can be empty, or
        // if the model was updated since the code was generated, it can have content that would
        // not be understood.
        if members.is_empty() {
            return Ok(());
        }
        let payload_member = members.iter().copied().find(|m| m.event_payload());
        let has_header_members = members.iter().any(|m| m.event_header());
        if payload_member.is_none() && !has_header_members {
            // No event traits at all: the whole struct is codec-encoded in the payload.
            let mut deser = self
                .capability
                .payload_codec
                .create_deserializer(&self.message.payload()[..]);
            return deser.read_struct(schema, state);
        }

        if has_header_members {
            for header in self.message.headers() {
                let name = header.name().as_str();
                let member = members
                    .iter()
                    .copied()
                    .find(|m| m.event_header() && m.member_name() == Some(name));
                match member {
                    Some(member) => {
                        state(member, &mut EventHeaderDeserializer { header })?;
                    }
                    // Event stream protocol headers start with ':'
                    None if !name.starts_with(':') => {
                        tracing::trace!("Unrecognized event stream message header: {}", name);
                    }
                    None => {}
                }
            }
        }

        if let Some(member) = payload_member {
            match member.shape_type() {
                ShapeType::Blob => {
                    self.check_content_type("application/octet-stream")?;
                    state(
                        member,
                        &mut RawPayloadDeserializer {
                            payload: self.message.payload(),
                        },
                    )?;
                }
                ShapeType::String => {
                    self.check_content_type("text/plain")?;
                    state(
                        member,
                        &mut RawPayloadDeserializer {
                            payload: self.message.payload(),
                        },
                    )?;
                }
                _ => {
                    let mut deser = self
                        .capability
                        .payload_codec
                        .create_deserializer(&self.message.payload()[..]);
                    state(member, &mut *deser)?;
                }
            }
        } else if members.iter().any(|m| !m.event_header() && !m.event_payload()) {
            // Members with neither trait are collectively codec-encoded in the payload,
            // per the Smithy spec. The codec sees a view of the schema without the
            // header-bound members: a payload key sharing a header member's name is then
            // unknown to the codec and skipped, so it cannot overwrite the value already
            // decoded from the message headers.
            let implicit_members: Vec<&Schema<'_>> =
                members.iter().copied().filter(|m| !m.event_header()).collect();
            let mut payload_schema =
                Schema::new_struct(schema.shape_id().clone(), schema.shape_type(), &implicit_members);
            // The filtered view must retain the XML document's root identity.
            if let Some(name) = schema.xml_name() {
                payload_schema = payload_schema.with_xml_name(name.value());
            }
            if let Some(namespace) = schema.xml_namespace() {
                payload_schema = payload_schema.with_xml_namespace(namespace.uri(), namespace.prefix());
            }
            if let Some(name) = schema.original_name() {
                payload_schema = payload_schema.with_original_name(name);
            }
            let mut deser = self
                .capability
                .payload_codec
                .create_deserializer(&self.message.payload()[..]);
            deser.read_struct(&payload_schema, state)?;
        }
        Ok(())
    }

    fn read_list(
        &mut self,
        _schema: &Schema<'_>,
        _state: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "an event stream frame deserializes a structure",
        ))
    }

    fn read_map(
        &mut self,
        _schema: &Schema<'_>,
        _state: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "an event stream frame deserializes a structure",
        ))
    }

    frames_deserialize_structs!(
        read_boolean() -> bool,
        read_byte() -> i8,
        read_short() -> i16,
        read_integer() -> i32,
        read_long() -> i64,
        read_float() -> f32,
        read_double() -> f64,
        read_big_integer() -> BigInteger,
        read_big_decimal() -> BigDecimal,
        read_string() -> String,
        read_blob() -> Blob,
        read_timestamp() -> DateTime,
        read_document() -> Document,
    );

    fn is_null(&self) -> bool {
        false
    }

    fn container_size(&self) -> Option<usize> {
        None
    }
}

/// Reads one `@eventHeader` member value from a message header.
struct EventHeaderDeserializer<'a> {
    header: &'a Header,
}

fn header_error(err: Error) -> SerdeError {
    SerdeError::custom(format!("{err}"))
}

macro_rules! typed_header_reads {
    ($($fn_name:ident($expect_fn:ident) -> $result_type:ty,)+) => {
        $(fn $fn_name(&mut self, _schema: &Schema<'_>) -> Result<$result_type, SerdeError> {
            expect_fns::$expect_fn(self.header).map_err(header_error)
        })+
    };
}

macro_rules! not_a_header_type {
    ($($fn_name:ident() -> $result_type:ty,)+) => {
        $(fn $fn_name(&mut self, _schema: &Schema<'_>) -> Result<$result_type, SerdeError> {
            Err(SerdeError::unsupported(
                "unsupported event stream header type",
            ))
        })+
    };
}

impl ShapeDeserializer for EventHeaderDeserializer<'_> {
    fn read_struct(
        &mut self,
        _schema: &Schema<'_>,
        _state: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "unsupported event stream header type",
        ))
    }

    fn read_list(
        &mut self,
        _schema: &Schema<'_>,
        _state: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "unsupported event stream header type",
        ))
    }

    fn read_map(
        &mut self,
        _schema: &Schema<'_>,
        _state: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "unsupported event stream header type",
        ))
    }

    typed_header_reads!(
        read_boolean(expect_bool) -> bool,
        read_byte(expect_byte) -> i8,
        read_short(expect_int16) -> i16,
        read_integer(expect_int32) -> i32,
        read_long(expect_int64) -> i64,
        read_string(expect_string) -> String,
        read_blob(expect_byte_array) -> Blob,
        read_timestamp(expect_timestamp) -> DateTime,
    );

    not_a_header_type!(
        read_float() -> f32,
        read_double() -> f64,
        read_big_integer() -> BigInteger,
        read_big_decimal() -> BigDecimal,
        read_document() -> Document,
    );

    fn is_null(&self) -> bool {
        false
    }

    fn container_size(&self) -> Option<usize> {
        None
    }
}

/// Reads a blob or string `@eventPayload` member directly from the message payload.
struct RawPayloadDeserializer<'a> {
    payload: &'a Bytes,
}

macro_rules! not_a_raw_payload_type {
    ($($fn_name:ident() -> $result_type:ty,)+) => {
        $(fn $fn_name(&mut self, _schema: &Schema<'_>) -> Result<$result_type, SerdeError> {
            Err(SerdeError::unsupported(
                "an event stream payload is a blob or a string",
            ))
        })+
    };
}

impl ShapeDeserializer for RawPayloadDeserializer<'_> {
    fn read_struct(
        &mut self,
        _schema: &Schema<'_>,
        _state: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "an event stream payload is a blob or a string",
        ))
    }

    fn read_list(
        &mut self,
        _schema: &Schema<'_>,
        _state: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "an event stream payload is a blob or a string",
        ))
    }

    fn read_map(
        &mut self,
        _schema: &Schema<'_>,
        _state: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "an event stream payload is a blob or a string",
        ))
    }

    fn read_string(&mut self, _schema: &Schema<'_>) -> Result<String, SerdeError> {
        Ok(std::str::from_utf8(self.payload)
            .map_err(|_| SerdeError::custom("message payload is not valid UTF-8"))?
            .to_owned())
    }

    fn read_blob(&mut self, _schema: &Schema<'_>) -> Result<Blob, SerdeError> {
        Ok(Blob::from_maybe_shared(self.payload.clone()))
    }

    not_a_raw_payload_type!(
        read_boolean() -> bool,
        read_byte() -> i8,
        read_short() -> i16,
        read_integer() -> i32,
        read_long() -> i64,
        read_float() -> f32,
        read_double() -> f64,
        read_big_integer() -> BigInteger,
        read_big_decimal() -> BigDecimal,
        read_timestamp() -> DateTime,
        read_document() -> Document,
    );

    fn is_null(&self) -> bool {
        false
    }

    fn container_size(&self) -> Option<usize> {
        None
    }
}

// ---------------------------------------------------------------------------
// Operation glue
// ---------------------------------------------------------------------------

/// Applies the `initial-request` frame, when the protocol carries one, to the operation input
/// being built.
///
/// `apply` receives a codec deserializer over the frame's payload and reads the non-stream
/// input members into the caller's builder. When the protocol does not frame initial messages,
/// or the stream starts with an ordinary event, nothing is consumed and `apply` is not called.
///
/// `recv_initial` reads the frame — pass the receiver's `try_recv_initial`, e.g.
/// `|message_type| receiver.try_recv_initial(message_type)`. Taking a closure rather than
/// `&mut Receiver` keeps this usable with generated receiver wrappers (such as the SigV4
/// unwrapping receiver) that expose the same method on a different type.
pub async fn apply_initial_request<Fut, Err>(
    recv_initial: impl FnOnce(InitialMessageType) -> Fut,
    protocol: &SharedServerProtocol,
    apply: impl FnOnce(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
) -> Result<(), DeserializeError>
where
    Fut: std::future::Future<Output = Result<Option<Message>, Err>>,
    Err: fmt::Display,
{
    let capability = protocol.event_stream_framing().ok_or_else(|| {
        DeserializeError::Serde(SerdeError::custom(NO_EVENT_STREAM_SUPPORT))
    })?;
    if !capability.initial_messages_in_frames {
        return Ok(());
    }
    match recv_initial(InitialMessageType::Request).await {
        Ok(Some(initial)) => {
            let mut deser = capability
                .payload_codec
                .create_deserializer(&initial.payload()[..]);
            apply(&mut *deser)?;
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(err) => Err(DeserializeError::Serde(SerdeError::custom(format!(
            "failed to read the initial-request frame: {err}"
        )))),
    }
}

/// Whether to emit an initial response before output events.
///
/// This policy is exhaustive: callers choose whether to send or omit the initial response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitialResponsePolicy {
    /// Emit an initial response only if the selected protocol supports initial-message framing.
    Send,
    /// Omit the initial response.
    Omit,
}

/// Builds the response body of an event stream output: the marshalled events, preceded by an
/// `initial-response` frame carrying the non-stream output members when the protocol frames
/// initial messages and `initial_response` is [`InitialResponsePolicy::Send`].
///
/// `output_schema` and `output` describe the non-stream members; the streaming member must
/// already have been moved out of `output` into `events`.
pub fn event_stream_response_body<T, E>(
    output_schema: &Schema<'_>,
    output: &dyn SerializableStruct,
    events: EventStreamSender<T, E>,
    marshaller: impl MarshallMessage<Input = T> + Send + Sync + 'static,
    error_marshaller: impl MarshallMessage<Input = E> + Send + Sync + 'static,
    protocol: &SharedServerProtocol,
    initial_response: InitialResponsePolicy,
) -> Result<BoxBody, SerdeError>
where
    T: Send + Sync + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    let capability = protocol
        .event_stream_framing()
        .ok_or_else(|| SerdeError::custom(NO_EVENT_STREAM_SUPPORT))?;
    let signer = NoOpSigner {};
    if capability.initial_messages_in_frames && initial_response == InitialResponsePolicy::Send {
        use futures_util::StreamExt;
        let payload = {
            let mut ser = capability.payload_codec.create_serializer();
            ser.write_struct(output_schema, output)?;
            Bytes::from(ser.finish_boxed())
        };
        let initial_message = Message::new_from_parts(
            vec![
                Header::new(":message-type", HeaderValue::String("event".into())),
                Header::new(":event-type", HeaderValue::String("initial-response".into())),
                Header::new(
                    ":content-type",
                    HeaderValue::String(capability.media_type.to_string().into()),
                ),
            ],
            payload,
        );
        let initial = futures_util::stream::iter([Ok(EventOrInitial::InitialMessage(
            initial_message,
        ))]);
        let events = events
            .into_inner()
            .map(|event| event.map(EventOrInitial::Event));
        let sender = EventStreamSender::from(initial.chain(events));
        let adapter = sender.into_body_stream(
            EventOrInitialMarshaller::new(marshaller),
            error_marshaller,
            signer,
        );
        Ok(boxed(http_body_util::StreamBody::new(adapter)))
    } else {
        let adapter = events.into_body_stream(marshaller, error_marshaller, signer);
        Ok(boxed(http_body_util::StreamBody::new(adapter)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_http::event_stream::Receiver;
    use crate::schema::protocol::RestJson1Protocol;
    use crate::schema::protocol::RpcV2CborProtocol;
    use aws_smithy_schema::ShapeId;

    fn json_protocol() -> SharedServerProtocol {
        SharedServerProtocol::metadata_routed(RestJson1Protocol::default())
    }

    fn cbor_protocol() -> SharedServerProtocol {
        SharedServerProtocol::metadata_routed(RpcV2CborProtocol::default())
    }

    fn header<'a>(message: &'a Message, name: &str) -> Option<&'a HeaderValue> {
        message
            .headers()
            .iter()
            .find(|h| h.name().as_str() == name)
            .map(|h| h.value())
    }

    fn string_header<'a>(message: &'a Message, name: &str) -> Option<&'a str> {
        header(message, name).map(|v| v.as_string().unwrap().as_str())
    }

    macro_rules! member_schema {
        ($name:ident, $shape:literal, $member:literal, $shape_type:expr, $index:literal $(, $with:ident)*) => {
            static $name: Schema<'static> = Schema::new_member(
                ShapeId::from_parts(
                    concat!("test#", $shape, "$", $member),
                    "test",
                    $shape,
                ),
                $shape_type,
                $member,
                $index,
            )$(.$with())*;
        };
    }

    macro_rules! struct_schema {
        ($name:ident, $shape:literal, [$($member:ident),*]) => {
            static $name: Schema<'static> = Schema::new_struct(
                ShapeId::from_parts(concat!("test#", $shape), "test", $shape),
                ShapeType::Structure,
                &[$(&$member),*],
            );
        };
    }

    // An event with every header type the event stream contract supports, one optional header
    // left `None`, and one member with no event trait at all.
    member_schema!(AH_BOOL, "AllHeaders", "flag", ShapeType::Boolean, 0, with_event_header);
    member_schema!(AH_BYTE, "AllHeaders", "small", ShapeType::Byte, 1, with_event_header);
    member_schema!(AH_SHORT, "AllHeaders", "short", ShapeType::Short, 2, with_event_header);
    member_schema!(AH_INT, "AllHeaders", "int", ShapeType::Integer, 3, with_event_header);
    member_schema!(AH_LONG, "AllHeaders", "long", ShapeType::Long, 4, with_event_header);
    member_schema!(AH_BLOB, "AllHeaders", "bin", ShapeType::Blob, 5, with_event_header);
    member_schema!(AH_STRING, "AllHeaders", "name", ShapeType::String, 6, with_event_header);
    member_schema!(AH_TIME, "AllHeaders", "at", ShapeType::Timestamp, 7, with_event_header);
    member_schema!(AH_SKIPPED, "AllHeaders", "skipped", ShapeType::String, 8, with_event_header);
    member_schema!(AH_EXTRA, "AllHeaders", "extra", ShapeType::String, 9);
    struct_schema!(
        ALL_HEADERS_SCHEMA,
        "AllHeaders",
        [AH_BOOL, AH_BYTE, AH_SHORT, AH_INT, AH_LONG, AH_BLOB, AH_STRING, AH_TIME, AH_SKIPPED, AH_EXTRA]
    );

    #[derive(Debug, Default, PartialEq)]
    struct AllHeaders {
        flag: Option<bool>,
        small: Option<i8>,
        short: Option<i16>,
        int: Option<i32>,
        long: Option<i64>,
        bin: Option<Blob>,
        name: Option<String>,
        at: Option<DateTime>,
        skipped: Option<String>,
        extra: Option<String>,
    }

    impl SerializableStruct for AllHeaders {
        fn schema(&self) -> &Schema<'_> {
            &ALL_HEADERS_SCHEMA
        }

        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            if let Some(v) = self.flag {
                ser.write_boolean(&AH_BOOL, v)?;
            }
            if let Some(v) = self.small {
                ser.write_byte(&AH_BYTE, v)?;
            }
            if let Some(v) = self.short {
                ser.write_short(&AH_SHORT, v)?;
            }
            if let Some(v) = self.int {
                ser.write_integer(&AH_INT, v)?;
            }
            if let Some(v) = self.long {
                ser.write_long(&AH_LONG, v)?;
            }
            if let Some(ref v) = self.bin {
                ser.write_blob(&AH_BLOB, v.clone())?;
            }
            if let Some(ref v) = self.name {
                ser.write_string(&AH_STRING, v)?;
            }
            if let Some(ref v) = self.at {
                ser.write_timestamp(&AH_TIME, v)?;
            }
            if let Some(ref v) = self.skipped {
                ser.write_string(&AH_SKIPPED, v)?;
            }
            if let Some(ref v) = self.extra {
                ser.write_string(&AH_EXTRA, v)?;
            }
            Ok(())
        }
    }

    impl AllHeaders {
        fn deserialize(deser: &mut dyn ShapeDeserializer) -> Result<Self, SerdeError> {
            let mut out = AllHeaders::default();
            deser.read_struct(&ALL_HEADERS_SCHEMA, &mut |member, d| {
                match member.member_index() {
                    Some(0) => out.flag = Some(d.read_boolean(member)?),
                    Some(1) => out.small = Some(d.read_byte(member)?),
                    Some(2) => out.short = Some(d.read_short(member)?),
                    Some(3) => out.int = Some(d.read_integer(member)?),
                    Some(4) => out.long = Some(d.read_long(member)?),
                    Some(5) => out.bin = Some(d.read_blob(member)?),
                    Some(6) => out.name = Some(d.read_string(member)?),
                    Some(7) => out.at = Some(d.read_timestamp(member)?),
                    Some(8) => out.skipped = Some(d.read_string(member)?),
                    Some(9) => {
                        if d.is_null() {
                            d.read_null()?;
                        } else {
                            out.extra = Some(d.read_string(member)?);
                        }
                    }
                    _ => {}
                }
                Ok(())
            })?;
            Ok(out)
        }
    }

    // A float `@eventHeader` is not representable; marshalling must fail.
    member_schema!(FH_RATE, "FloatHeader", "rate", ShapeType::Float, 0, with_event_header);
    struct_schema!(FLOAT_HEADER_SCHEMA, "FloatHeader", [FH_RATE]);

    #[derive(Debug)]
    struct FloatHeader {
        rate: f32,
    }

    impl SerializableStruct for FloatHeader {
        fn schema(&self) -> &Schema<'_> {
            &FLOAT_HEADER_SCHEMA
        }

        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            ser.write_float(&FH_RATE, self.rate)
        }
    }

    // String and blob `@eventPayload` members carry raw bytes with fixed content types.
    member_schema!(TEXT_VALUE, "TextEvent", "value", ShapeType::String, 0, with_event_payload);
    struct_schema!(TEXT_EVENT_SCHEMA, "TextEvent", [TEXT_VALUE]);

    #[derive(Debug, Default, PartialEq)]
    struct TextEvent {
        value: Option<String>,
    }

    impl SerializableStruct for TextEvent {
        fn schema(&self) -> &Schema<'_> {
            &TEXT_EVENT_SCHEMA
        }

        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            if let Some(ref v) = self.value {
                ser.write_string(&TEXT_VALUE, v)?;
            }
            Ok(())
        }
    }

    impl TextEvent {
        fn deserialize(deser: &mut dyn ShapeDeserializer) -> Result<Self, SerdeError> {
            let mut out = TextEvent::default();
            deser.read_struct(&TEXT_EVENT_SCHEMA, &mut |member, d| {
                if member.member_index() == Some(0) {
                    out.value = Some(d.read_string(member)?);
                }
                Ok(())
            })?;
            Ok(out)
        }
    }

    member_schema!(BIN_VALUE, "BinEvent", "value", ShapeType::Blob, 0, with_event_payload);
    struct_schema!(BIN_EVENT_SCHEMA, "BinEvent", [BIN_VALUE]);

    #[derive(Debug, Default, PartialEq)]
    struct BinEvent {
        value: Option<Blob>,
    }

    impl SerializableStruct for BinEvent {
        fn schema(&self) -> &Schema<'_> {
            &BIN_EVENT_SCHEMA
        }

        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            if let Some(ref v) = self.value {
                ser.write_blob(&BIN_VALUE, v.clone())?;
            }
            Ok(())
        }
    }

    impl BinEvent {
        fn deserialize(deser: &mut dyn ShapeDeserializer) -> Result<Self, SerdeError> {
            let mut out = BinEvent::default();
            deser.read_struct(&BIN_EVENT_SCHEMA, &mut |member, d| {
                if member.member_index() == Some(0) {
                    out.value = Some(d.read_blob(member)?);
                }
                Ok(())
            })?;
            Ok(out)
        }
    }

    // A structured `@eventPayload` goes through the protocol's codec, alongside a header.
    member_schema!(BODY_TEXT, "MessageBody", "text", ShapeType::String, 0);
    struct_schema!(MESSAGE_BODY_SCHEMA, "MessageBody", [BODY_TEXT]);

    #[derive(Debug, Default, PartialEq)]
    struct MessageBody {
        text: Option<String>,
    }

    impl SerializableStruct for MessageBody {
        fn schema(&self) -> &Schema<'_> {
            &MESSAGE_BODY_SCHEMA
        }

        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            if let Some(ref v) = self.text {
                ser.write_string(&BODY_TEXT, v)?;
            }
            Ok(())
        }
    }

    impl MessageBody {
        fn deserialize(deser: &mut dyn ShapeDeserializer) -> Result<Self, SerdeError> {
            let mut out = MessageBody::default();
            deser.read_struct(&MESSAGE_BODY_SCHEMA, &mut |member, d| {
                if member.member_index() == Some(0) {
                    out.text = Some(d.read_string(member)?);
                }
                Ok(())
            })?;
            Ok(out)
        }
    }

    member_schema!(STRUCT_FROM, "StructEvent", "from", ShapeType::String, 0, with_event_header);
    member_schema!(STRUCT_BODY, "StructEvent", "body", ShapeType::Structure, 1, with_event_payload);
    struct_schema!(STRUCT_EVENT_SCHEMA, "StructEvent", [STRUCT_FROM, STRUCT_BODY]);

    #[derive(Debug, Default, PartialEq)]
    struct StructEvent {
        from: Option<String>,
        body: Option<MessageBody>,
    }

    impl SerializableStruct for StructEvent {
        fn schema(&self) -> &Schema<'_> {
            &STRUCT_EVENT_SCHEMA
        }

        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            if let Some(ref v) = self.from {
                ser.write_string(&STRUCT_FROM, v)?;
            }
            if let Some(ref v) = self.body {
                ser.write_struct(&STRUCT_BODY, v)?;
            }
            Ok(())
        }
    }

    impl StructEvent {
        fn deserialize(deser: &mut dyn ShapeDeserializer) -> Result<Self, SerdeError> {
            let mut out = StructEvent::default();
            deser.read_struct(&STRUCT_EVENT_SCHEMA, &mut |member, d| {
                match member.member_index() {
                    Some(0) => out.from = Some(d.read_string(member)?),
                    Some(1) => out.body = Some(MessageBody::deserialize(d)?),
                    _ => {}
                }
                Ok(())
            })?;
            Ok(out)
        }
    }

    // No members at all: the payload is skipped entirely on the way in.
    struct_schema!(EMPTY_EVENT_SCHEMA, "EmptyEvent", []);

    #[derive(Debug, Default, PartialEq)]
    struct EmptyEvent;

    impl SerializableStruct for EmptyEvent {
        fn schema(&self) -> &Schema<'_> {
            &EMPTY_EVENT_SCHEMA
        }

        fn serialize_members(&self, _ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            Ok(())
        }
    }

    // No event traits: the whole struct is the codec-encoded payload.
    member_schema!(PLAIN_TEXT, "PlainEvent", "text", ShapeType::String, 0);
    struct_schema!(PLAIN_EVENT_SCHEMA, "PlainEvent", [PLAIN_TEXT]);

    #[derive(Debug, Default, PartialEq)]
    struct PlainEvent {
        text: Option<String>,
    }

    impl SerializableStruct for PlainEvent {
        fn schema(&self) -> &Schema<'_> {
            &PLAIN_EVENT_SCHEMA
        }

        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            if let Some(ref v) = self.text {
                ser.write_string(&PLAIN_TEXT, v)?;
            }
            Ok(())
        }
    }

    impl PlainEvent {
        fn deserialize(deser: &mut dyn ShapeDeserializer) -> Result<Self, SerdeError> {
            let mut out = PlainEvent::default();
            deser.read_struct(&PLAIN_EVENT_SCHEMA, &mut |member, d| {
                if member.member_index() == Some(0) {
                    out.text = Some(d.read_string(member)?);
                }
                Ok(())
            })?;
            Ok(out)
        }
    }

    // The stream union: one member schema per event, named by its Smithy member name.
    member_schema!(EV_ALL_HEADERS, "TestEvents", "allHeaders", ShapeType::Structure, 0);
    member_schema!(EV_FLOAT_HEADER, "TestEvents", "floatHeader", ShapeType::Structure, 1);
    member_schema!(EV_TEXT, "TestEvents", "text", ShapeType::Structure, 2);
    member_schema!(EV_BIN, "TestEvents", "bin", ShapeType::Structure, 3);
    member_schema!(EV_STRUCTURED, "TestEvents", "structured", ShapeType::Structure, 4);
    member_schema!(EV_EMPTY, "TestEvents", "empty", ShapeType::Structure, 5);
    member_schema!(EV_PLAIN, "TestEvents", "plain", ShapeType::Structure, 6);

    #[derive(Debug, PartialEq)]
    enum TestEvents {
        AllHeaders(AllHeaders),
        FloatHeader(FloatHeader),
        Text(TextEvent),
        Bin(BinEvent),
        Structured(StructEvent),
        Empty(EmptyEvent),
        Plain(PlainEvent),
    }

    impl PartialEq for FloatHeader {
        fn eq(&self, other: &Self) -> bool {
            self.rate == other.rate
        }
    }

    static TEST_EVENTS_SCHEMA: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#TestEvents", "test", "TestEvents"),
        ShapeType::Union,
        &[
            &EV_ALL_HEADERS,
            &EV_FLOAT_HEADER,
            &EV_TEXT,
            &EV_BIN,
            &EV_STRUCTURED,
            &EV_EMPTY,
            &EV_PLAIN,
        ],
    )
    .with_streaming();

    impl SerializableStruct for TestEvents {
        fn schema(&self) -> &Schema<'_> {
            &TEST_EVENTS_SCHEMA
        }

        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            match self {
                Self::AllHeaders(v) => ser.write_struct(&EV_ALL_HEADERS, v),
                Self::FloatHeader(v) => ser.write_struct(&EV_FLOAT_HEADER, v),
                Self::Text(v) => ser.write_struct(&EV_TEXT, v),
                Self::Bin(v) => ser.write_struct(&EV_BIN, v),
                Self::Structured(v) => ser.write_struct(&EV_STRUCTURED, v),
                Self::Empty(v) => ser.write_struct(&EV_EMPTY, v),
                Self::Plain(v) => ser.write_struct(&EV_PLAIN, v),
            }
        }
    }

    #[derive(Debug)]
    enum TestEventsError {
        Boom(BoomError),
    }

    impl fmt::Display for TestEventsError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "boom")
        }
    }

    impl std::error::Error for TestEventsError {}

    member_schema!(BOOM_MESSAGE, "BoomError", "message", ShapeType::String, 0);
    struct_schema!(BOOM_ERROR_SCHEMA, "BoomError", [BOOM_MESSAGE]);

    #[derive(Debug, Default, PartialEq)]
    struct BoomError {
        message: Option<String>,
    }

    impl SerializableStruct for BoomError {
        fn schema(&self) -> &Schema<'_> {
            &BOOM_ERROR_SCHEMA
        }

        fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            if let Some(ref v) = self.message {
                ser.write_string(&BOOM_MESSAGE, v)?;
            }
            Ok(())
        }
    }

    impl BoomError {
        fn deserialize(deser: &mut dyn ShapeDeserializer) -> Result<Self, SerdeError> {
            let mut out = BoomError::default();
            deser.read_struct(&BOOM_ERROR_SCHEMA, &mut |member, d| {
                if member.member_index() == Some(0) {
                    out.message = Some(d.read_string(member)?);
                }
                Ok(())
            })?;
            Ok(out)
        }
    }

    impl SerializableEventError for TestEventsError {
        fn variant(&self) -> (&'static str, &dyn SerializableStruct) {
            match self {
                Self::Boom(inner) => ("boom", inner),
            }
        }
    }

    impl DeserializableEventStream for TestEvents {
        type Error = TestEventsError;

        fn deserialize_event(
            event_type: &str,
            frame: &EventFrame<'_>,
        ) -> Result<Option<Self>, Error> {
            let mut deser = frame.deserializer();
            let wrap = |err: SerdeError| {
                Error::unmarshalling(format!("failed to unmarshall {event_type}: {err}"))
            };
            Ok(Some(match event_type {
                "allHeaders" => Self::AllHeaders(AllHeaders::deserialize(&mut deser).map_err(wrap)?),
                "text" => Self::Text(TextEvent::deserialize(&mut deser).map_err(wrap)?),
                "bin" => Self::Bin(BinEvent::deserialize(&mut deser).map_err(wrap)?),
                "structured" => Self::Structured(StructEvent::deserialize(&mut deser).map_err(wrap)?),
                "empty" => Self::Empty(EmptyEvent),
                "plain" => Self::Plain(PlainEvent::deserialize(&mut deser).map_err(wrap)?),
                _ => return Ok(None),
            }))
        }

        fn deserialize_error(
            exception_type: &str,
            frame: &EventFrame<'_>,
        ) -> Result<Option<Self::Error>, Error> {
            match exception_type {
                "boom" => {
                    let mut deser = frame.deserializer();
                    let parsed = BoomError::deserialize(&mut deser).map_err(|err| {
                        Error::unmarshalling(format!("failed to unmarshall exception: {err}"))
                    })?;
                    Ok(Some(TestEventsError::Boom(parsed)))
                }
                _ => Ok(None),
            }
        }
    }

    fn marshall(protocol: &SharedServerProtocol, event: TestEvents) -> Message {
        SchemaEventMarshaller::<TestEvents>::new(protocol.clone())
            .marshall(event)
            .expect("marshalls")
    }

    fn unmarshall(
        protocol: &SharedServerProtocol,
        message: &Message,
    ) -> Result<UnmarshalledMessage<TestEvents, TestEventsError>, Error> {
        SchemaEventUnmarshaller::<TestEvents>::new(protocol.clone()).unmarshall(message)
    }

    #[test]
    fn all_header_types_round_trip_and_keep_declaration_order() {
        let protocol = json_protocol();
        let event = AllHeaders {
            flag: Some(true),
            small: Some(-3),
            short: Some(-300),
            int: Some(70_000),
            long: Some(5_000_000_000),
            bin: Some(Blob::new(&b"\x00\xff"[..])),
            name: Some("ann".to_owned()),
            at: Some(DateTime::from_secs(1_700_000_000)),
            skipped: None,
            extra: Some("payload".to_owned()),
        };
        let message = marshall(&protocol, TestEvents::AllHeaders(event));

        let names: Vec<_> = message.headers().iter().map(|h| h.name().as_str()).collect();
        // `skipped` is absent; `extra` goes into the document, not the headers.
        assert_eq!(
            names,
            [":message-type", ":event-type", "flag", "small", "short", "int", "long", "bin", "name", "at", ":content-type"]
        );
        assert_eq!(string_header(&message, ":event-type"), Some("allHeaders"));
        assert_eq!(&message.payload()[..], br#"{"extra":"payload"}"#);
        assert!(matches!(header(&message, "flag"), Some(HeaderValue::Bool(true))));
        assert!(matches!(header(&message, "small"), Some(HeaderValue::Byte(-3))));
        assert!(matches!(header(&message, "short"), Some(HeaderValue::Int16(-300))));
        assert!(matches!(header(&message, "int"), Some(HeaderValue::Int32(70_000))));
        assert!(matches!(header(&message, "long"), Some(HeaderValue::Int64(5_000_000_000))));
        assert!(matches!(header(&message, "bin"), Some(HeaderValue::ByteArray(b)) if &b[..] == b"\x00\xff"));
        assert!(matches!(header(&message, "at"), Some(HeaderValue::Timestamp(_))));

        match unmarshall(&protocol, &message).expect("unmarshalls") {
            UnmarshalledMessage::Event(TestEvents::AllHeaders(parsed)) => {
                assert_eq!(parsed.flag, Some(true));
                assert_eq!(parsed.small, Some(-3));
                assert_eq!(parsed.short, Some(-300));
                assert_eq!(parsed.int, Some(70_000));
                assert_eq!(parsed.long, Some(5_000_000_000));
                assert_eq!(parsed.bin.as_ref().map(|b| b.as_ref()), Some(&b"\x00\xff"[..]));
                assert_eq!(parsed.name.as_deref(), Some("ann"));
                assert_eq!(parsed.at, Some(DateTime::from_secs(1_700_000_000)));
                assert_eq!(parsed.skipped, None);
                assert_eq!(parsed.extra.as_deref(), Some("payload"));
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn implicit_payload_uses_selected_codec_and_preserves_wire_names() {
        static HEADER: Schema<'static> = Schema::new_member(
            ShapeId::from_parts("test#Implicit$header", "test", "Implicit"),
            ShapeType::String,
            "header",
            0,
        )
        .with_event_header();
        static DATA: Schema<'static> = Schema::new_member(
            ShapeId::from_parts("test#Implicit$data", "test", "Implicit"),
            ShapeType::String,
            "data",
            1,
        )
        .with_json_name("wireData")
        .with_xml_name("WireData");
        static SCHEMA: Schema<'static> = Schema::new_struct(
            ShapeId::from_parts("test#Implicit", "test", "Implicit"),
            ShapeType::Structure,
            &[&HEADER, &DATA],
        )
        .with_xml_name("WireEvent")
        .with_xml_namespace("urn:test", None);
        struct Implicit;
        impl SerializableStruct for Implicit {
            fn schema(&self) -> &Schema<'_> {
                &SCHEMA
            }
            fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                ser.write_string(&HEADER, "ann")?;
                ser.write_string(&DATA, "hi")
            }
        }
        for (protocol, media_type, expected) in [
            (json_protocol(), "application/json", br#"{"wireData":"hi"}"#.as_slice()),
            (
                cbor_protocol(),
                "application/cbor",
                b"\xbf\x64data\x62hi\xff".as_slice(),
            ),
            (
                SharedServerProtocol::metadata_routed(crate::schema::protocol::RestXmlProtocol::default()),
                "application/xml",
                br#"<WireEvent xmlns="urn:test"><WireData>hi</WireData></WireEvent>"#.as_slice(),
            ),
        ] {
            let capability = protocol.event_stream_framing().unwrap();
            let message = build_frame(capability, "implicit", &Implicit, FrameKind::Event).unwrap();
            assert_eq!(string_header(&message, "header"), Some("ann"));
            assert_eq!(string_header(&message, ":content-type"), Some(media_type));
            assert_eq!(message.payload().as_ref(), expected, "{media_type}");
            let mut values = [None, None];
            EventFrame::new(&message, capability)
                .deserializer()
                .read_struct(&SCHEMA, &mut |member, d| {
                    values[member.member_index().unwrap()] = Some(d.read_string(member)?);
                    Ok(())
                })
                .unwrap();
            assert_eq!(values, [Some("ann".to_owned()), Some("hi".to_owned())]);
        }
    }

    #[test]
    fn header_only_event_has_no_payload_or_content_type() {
        static SCHEMA: Schema<'static> = Schema::new_struct(
            ShapeId::from_parts("test#HeaderOnly", "test", "HeaderOnly"),
            ShapeType::Structure,
            &[&AH_STRING],
        );
        struct HeaderOnly;
        impl SerializableStruct for HeaderOnly {
            fn schema(&self) -> &Schema<'_> {
                &SCHEMA
            }
            fn serialize_members(&self, ser: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                ser.write_string(&AH_STRING, "ann")
            }
        }
        let protocol = json_protocol();
        let capability = protocol.event_stream_framing().unwrap();
        let message = build_frame(capability, "headerOnly", &HeaderOnly, FrameKind::Event).unwrap();
        assert_eq!(string_header(&message, "name"), Some("ann"));
        assert_eq!(string_header(&message, ":content-type"), None);
        assert!(message.payload().is_empty());
    }

    #[test]
    fn absent_implicit_members_still_form_a_payload_document() {
        let protocol = json_protocol();
        let message = marshall(
            &protocol,
            TestEvents::AllHeaders(AllHeaders {
                name: Some("ann".to_owned()),
                ..Default::default()
            }),
        );
        assert_eq!(message.payload().as_ref(), b"{}");
        assert_eq!(string_header(&message, ":content-type"), Some("application/json"));
        match unmarshall(&protocol, &message).unwrap() {
            UnmarshalledMessage::Event(TestEvents::AllHeaders(parsed)) => {
                assert_eq!(parsed.name.as_deref(), Some("ann"));
                assert_eq!(parsed.extra, None);
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn float_header_fails_to_marshall() {
        let err = SchemaEventMarshaller::<TestEvents>::new(json_protocol())
            .marshall(TestEvents::FloatHeader(FloatHeader { rate: 1.5 }))
            .expect_err("floats are not event stream header types");
        assert!(err.to_string().contains("unsupported event stream header type"));
    }

    #[test]
    fn string_payload_is_raw_text() {
        let protocol = json_protocol();
        let message = marshall(
            &protocol,
            TestEvents::Text(TextEvent {
                value: Some("raw string".to_owned()),
            }),
        );
        assert_eq!(string_header(&message, ":content-type"), Some("text/plain"));
        assert_eq!(&message.payload()[..], b"raw string");
        match unmarshall(&protocol, &message).expect("unmarshalls") {
            UnmarshalledMessage::Event(TestEvents::Text(parsed)) => {
                assert_eq!(parsed.value.as_deref(), Some("raw string"));
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn optional_string_payload_none_is_empty_with_content_type() {
        let message = marshall(&json_protocol(), TestEvents::Text(TextEvent { value: None }));
        assert_eq!(string_header(&message, ":content-type"), Some("text/plain"));
        assert!(message.payload().is_empty());
    }

    #[test]
    fn blob_payload_is_raw_bytes() {
        let protocol = json_protocol();
        let message = marshall(
            &protocol,
            TestEvents::Bin(BinEvent {
                value: Some(Blob::new(&b"\x00\xff"[..])),
            }),
        );
        assert_eq!(string_header(&message, ":content-type"), Some("application/octet-stream"));
        assert_eq!(&message.payload()[..], b"\x00\xff");
        match unmarshall(&protocol, &message).expect("unmarshalls") {
            UnmarshalledMessage::Event(TestEvents::Bin(parsed)) => {
                assert_eq!(parsed.value.as_ref().map(|b| b.as_ref()), Some(&b"\x00\xff"[..]));
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn structured_payload_goes_through_the_selected_codec() {
        for (protocol, media_type) in [
            (json_protocol(), "application/json"),
            (cbor_protocol(), "application/cbor"),
        ] {
            let message = marshall(
                &protocol,
                TestEvents::Structured(StructEvent {
                    from: Some("ann".to_owned()),
                    body: Some(MessageBody {
                        text: Some("hi".to_owned()),
                    }),
                }),
            );
            // Header order: prelude headers, member headers in write order, `:content-type` last.
            let names: Vec<_> = message.headers().iter().map(|h| h.name().as_str()).collect();
            assert_eq!(names, [":message-type", ":event-type", "from", ":content-type"]);
            assert_eq!(string_header(&message, ":content-type"), Some(media_type));
            if media_type == "application/json" {
                assert_eq!(&message.payload()[..], br#"{"text":"hi"}"#);
            }
            match unmarshall(&protocol, &message).expect("unmarshalls") {
                UnmarshalledMessage::Event(TestEvents::Structured(parsed)) => {
                    assert_eq!(parsed.from.as_deref(), Some("ann"));
                    assert_eq!(parsed.body.and_then(|b| b.text).as_deref(), Some("hi"));
                }
                other => panic!("unexpected result: {other:?}"),
            }
        }
    }

    #[test]
    fn optional_structured_payload_none_is_empty_with_content_type() {
        let message = marshall(
            &json_protocol(),
            TestEvents::Structured(StructEvent {
                from: Some("ann".to_owned()),
                body: None,
            }),
        );
        assert_eq!(string_header(&message, ":content-type"), Some("application/json"));
        assert!(message.payload().is_empty());
    }

    #[test]
    fn no_event_traits_serializes_the_whole_struct_as_payload() {
        let protocol = json_protocol();
        let message = marshall(
            &protocol,
            TestEvents::Plain(PlainEvent {
                text: Some("hi".to_owned()),
            }),
        );
        assert_eq!(string_header(&message, ":content-type"), Some("application/json"));
        assert_eq!(&message.payload()[..], br#"{"text":"hi"}"#);
        match unmarshall(&protocol, &message).expect("unmarshalls") {
            UnmarshalledMessage::Event(TestEvents::Plain(parsed)) => {
                assert_eq!(parsed.text.as_deref(), Some("hi"));
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn empty_struct_serializes_as_codec_payload_and_skips_it_on_read() {
        let protocol = json_protocol();
        let message = marshall(&protocol, TestEvents::Empty(EmptyEvent));
        assert_eq!(string_header(&message, ":content-type"), Some("application/json"));
        assert_eq!(&message.payload()[..], br#"{}"#);
        // Reading back skips the payload entirely, even if it is garbage.
        let garbage = Message::new_from_parts(message.headers().to_vec(), Bytes::from_static(b"!!"));
        assert!(matches!(
            unmarshall(&protocol, &garbage).expect("unmarshalls"),
            UnmarshalledMessage::Event(TestEvents::Empty(_))
        ));
    }

    #[test]
    fn untraited_members_alongside_headers_are_read_from_the_payload() {
        // A peer-supplied payload document supplies ordinary members alongside headers.
        let protocol = json_protocol();
        let message = Message::new_from_parts(
            vec![
                Header::new(":message-type", HeaderValue::String("event".into())),
                Header::new(":event-type", HeaderValue::String("allHeaders".into())),
                Header::new("name", HeaderValue::String("ann".into())),
                Header::new("unknown", HeaderValue::String("traced, not fatal".into())),
            ],
            Bytes::from_static(br#"{"extra":"from payload"}"#),
        );
        match unmarshall(&protocol, &message).expect("unmarshalls") {
            UnmarshalledMessage::Event(TestEvents::AllHeaders(parsed)) => {
                assert_eq!(parsed.name.as_deref(), Some("ann"));
                assert_eq!(parsed.extra.as_deref(), Some("from payload"));
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn payload_cannot_overwrite_event_headers() {
        // A payload key that shares an `@eventHeader` member's name is ignored: the
        // header-decoded value wins, matching the generated unmarshallers, which copied
        // only the implicit payload members.
        let protocol = json_protocol();
        let message = Message::new_from_parts(
            vec![
                Header::new(":message-type", HeaderValue::String("event".into())),
                Header::new(":event-type", HeaderValue::String("allHeaders".into())),
                Header::new("name", HeaderValue::String("ann".into())),
            ],
            Bytes::from_static(br#"{"name":"bob","extra":"from payload"}"#),
        );
        match unmarshall(&protocol, &message).expect("unmarshalls") {
            UnmarshalledMessage::Event(TestEvents::AllHeaders(parsed)) => {
                assert_eq!(parsed.name.as_deref(), Some("ann"));
                assert_eq!(parsed.extra.as_deref(), Some("from payload"));
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn unmarshalling_error_strings() {
        let protocol = json_protocol();

        let bogus_message_type = Message::new_from_parts(
            vec![
                Header::new(":message-type", HeaderValue::String("bogus".into())),
                Header::new(":event-type", HeaderValue::String("text".into())),
            ],
            Bytes::new(),
        );
        // `parse_response_headers` rejects the unknown type before the dispatch does, exactly
        // as it did on the generated path.
        assert_eq!(
            unmarshall(&protocol, &bogus_message_type).unwrap_err().to_string(),
            "failed to unmarshall message: unrecognized `:message-type`: bogus"
        );

        let unknown_event = Message::new_from_parts(
            vec![
                Header::new(":message-type", HeaderValue::String("event".into())),
                Header::new(":event-type", HeaderValue::String("nope".into())),
            ],
            Bytes::new(),
        );
        assert_eq!(
            unmarshall(&protocol, &unknown_event).unwrap_err().to_string(),
            "failed to unmarshall message: unrecognized :event-type: nope"
        );

        let unknown_exception = Message::new_from_parts(
            vec![
                Header::new(":message-type", HeaderValue::String("exception".into())),
                Header::new(":exception-type", HeaderValue::String("nope".into())),
            ],
            Bytes::new(),
        );
        assert_eq!(
            unmarshall(&protocol, &unknown_exception).unwrap_err().to_string(),
            "failed to unmarshall message: unrecognized exception: nope"
        );

        let wrong_content_type = Message::new_from_parts(
            vec![
                Header::new(":message-type", HeaderValue::String("event".into())),
                Header::new(":event-type", HeaderValue::String("text".into())),
                Header::new(":content-type", HeaderValue::String("application/json".into())),
            ],
            Bytes::from_static(b"raw"),
        );
        assert!(unmarshall(&protocol, &wrong_content_type)
            .unwrap_err()
            .to_string()
            .contains("expected :content-type to be 'text/plain', but was 'application/json'"));

        let invalid_utf8 = Message::new_from_parts(
            vec![
                Header::new(":message-type", HeaderValue::String("event".into())),
                Header::new(":event-type", HeaderValue::String("text".into())),
                Header::new(":content-type", HeaderValue::String("text/plain".into())),
            ],
            Bytes::from_static(b"\xff\xfe"),
        );
        assert!(unmarshall(&protocol, &invalid_utf8)
            .unwrap_err()
            .to_string()
            .contains("message payload is not valid UTF-8"));
    }

    #[test]
    fn modeled_error_marshals_as_exception_frame() {
        let protocol = json_protocol();
        let marshaller = SchemaEventErrorMarshaller::<TestEventsError>::new(protocol.clone());
        let message = marshaller
            .marshall(TestEventsError::Boom(BoomError {
                message: Some("failure".to_owned()),
            }))
            .expect("marshalls");
        assert_eq!(string_header(&message, ":message-type"), Some("exception"));
        assert_eq!(string_header(&message, ":exception-type"), Some("boom"));
        assert_eq!(string_header(&message, ":content-type"), Some("application/json"));
        match unmarshall(&protocol, &message).expect("unmarshalls") {
            UnmarshalledMessage::Error(TestEventsError::Boom(parsed)) => {
                assert_eq!(parsed.message.as_deref(), Some("failure"));
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn unmodeled_error_marshaller_emits_a_bare_exception_frame() {
        let marshaller = NoModeledEventErrorMarshaller::new(json_protocol());
        let message = marshaller
            .marshall(aws_smithy_http::event_stream::MessageStreamError::unhandled(
                std::io::Error::other("boom"),
            ))
            .expect("marshalls");
        let names: Vec<_> = message.headers().iter().map(|h| h.name().as_str()).collect();
        assert_eq!(names, [":message-type"]);
        assert_eq!(string_header(&message, ":message-type"), Some("exception"));
        assert!(message.payload().is_empty());
    }

    /// A protocol with no event stream capability rejects every operation up front.
    #[test]
    fn protocols_without_event_stream_support_are_rejected() {
        #[derive(Debug)]
        struct HttpOnly;
        impl crate::schema::ServerProtocol for HttpOnly {
            fn protocol_id(&self) -> &'static ShapeId<'static> {
                static ID: ShapeId<'static> = aws_smithy_schema::shape_id!("test", "HttpOnly");
                &ID
            }
            fn deserialize_request<'a>(
                &'a self,
                _: &Schema<'_>,
                _: &'a aws_smithy_runtime_api::http::Request<bytes::Bytes>,
            ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
                unreachable!()
            }
            fn serialize_response(
                &self,
                _: &Schema<'_>,
                _: &dyn SerializableStruct,
            ) -> crate::response::Response {
                unreachable!()
            }
            fn serialize_streaming_response(
                &self,
                _: &Schema<'_>,
                _: &dyn SerializableStruct,
                _: BoxBody,
            ) -> crate::response::Response {
                unreachable!()
            }
            fn serialize_error(
                &self,
                _: &dyn crate::schema::HttpModeledError,
            ) -> crate::response::Response {
                unreachable!()
            }
            fn serialize_rejection(&self, _: DeserializeError) -> crate::response::Response {
                unreachable!()
            }
        }

        let protocol = SharedServerProtocol::serde_only(HttpOnly);
        assert!(SchemaEventMarshaller::<TestEvents>::new(protocol.clone())
            .marshall(TestEvents::Empty(EmptyEvent))
            .is_err());
        assert!(SchemaEventMarshaller::<TestEvents>::new(protocol.clone())
            .marshall(TestEvents::Text(TextEvent {
                value: Some("raw".to_owned())
            }))
            .is_err());
        assert!(SchemaEventErrorMarshaller::<TestEventsError>::new(protocol.clone())
            .marshall(TestEventsError::Boom(BoomError { message: None }))
            .is_err());
        assert!(NoModeledEventErrorMarshaller::new(protocol.clone())
            .marshall(aws_smithy_http::event_stream::MessageStreamError::unhandled(
                std::io::Error::other("boom"),
            ))
            .is_err());
        assert!(SchemaEventUnmarshaller::<TestEvents>::new(protocol)
            .unmarshall(&Message::new(Bytes::new()))
            .is_err());
    }

    // ---- operation glue ----

    fn write_frame(message: &Message, out: &mut Vec<u8>) {
        aws_smithy_eventstream::frame::write_message_to(message, out).expect("writes");
    }

    fn initial_request_frame(payload: &'static [u8], content_type: &str) -> Message {
        Message::new_from_parts(
            vec![
                Header::new(":message-type", HeaderValue::String("event".into())),
                Header::new(":event-type", HeaderValue::String("initial-request".into())),
                Header::new(":content-type", HeaderValue::String(content_type.to_owned().into())),
            ],
            Bytes::from_static(payload),
        )
    }

    #[tokio::test]
    async fn apply_initial_request_reads_the_initial_frame() {
        let protocol = cbor_protocol();
        // CBOR map { "text": "hi" }, followed by an ordinary event.
        let mut body = Vec::new();
        write_frame(
            &initial_request_frame(b"\xa1\x64text\x62hi", "application/cbor"),
            &mut body,
        );
        write_frame(
            &marshall(&protocol, TestEvents::Text(TextEvent { value: Some("evt".to_owned()) })),
            &mut body,
        );
        let unmarshaller = SchemaEventUnmarshaller::<TestEvents>::new(protocol.clone());
        let mut receiver = Receiver::new(unmarshaller, aws_smithy_types::body::SdkBody::from(body));
        let mut applied = None;
        apply_initial_request(|mt| receiver.try_recv_initial(mt), &protocol, |deser| {
            applied = Some(PlainEvent::deserialize(deser)?);
            Ok(())
        })
        .await
        .expect("applies");
        assert_eq!(applied.and_then(|p| p.text).as_deref(), Some("hi"));
        // The ordinary event that follows is preserved.
        let event = receiver.recv().await.expect("a frame").expect("an event");
        assert_eq!(event, TestEvents::Text(TextEvent { value: Some("evt".to_owned()) }));
    }

    #[tokio::test]
    async fn apply_initial_request_preserves_a_first_ordinary_event() {
        let protocol = cbor_protocol();
        let mut body = Vec::new();
        write_frame(
            &marshall(&protocol, TestEvents::Text(TextEvent { value: Some("evt".to_owned()) })),
            &mut body,
        );
        let unmarshaller = SchemaEventUnmarshaller::<TestEvents>::new(protocol.clone());
        let mut receiver = Receiver::new(unmarshaller, aws_smithy_types::body::SdkBody::from(body));
        apply_initial_request(|mt| receiver.try_recv_initial(mt), &protocol, |_| {
            panic!("no initial frame to apply")
        })
        .await
        .expect("no initial frame is fine");
        let event = receiver.recv().await.expect("a frame").expect("an event");
        assert_eq!(event, TestEvents::Text(TextEvent { value: Some("evt".to_owned()) }));
    }

    #[tokio::test]
    async fn apply_initial_request_skips_protocols_without_initial_frames() {
        let protocol = json_protocol();
        let unmarshaller = SchemaEventUnmarshaller::<TestEvents>::new(protocol.clone());
        let mut receiver = Receiver::new(
            unmarshaller,
            aws_smithy_types::body::SdkBody::from(Vec::<u8>::new()),
        );
        apply_initial_request(|mt| receiver.try_recv_initial(mt), &protocol, |_| {
            panic!("restJson1 carries no initial frames")
        })
        .await
        .expect("nothing to do");
    }

    #[tokio::test]
    async fn apply_initial_request_reports_malformed_frames() {
        let protocol = cbor_protocol();
        let unmarshaller = SchemaEventUnmarshaller::<TestEvents>::new(protocol.clone());
        let mut receiver = Receiver::new(
            unmarshaller,
            aws_smithy_types::body::SdkBody::from(&b"malformed frame"[..]),
        );
        let err = apply_initial_request(|mt| receiver.try_recv_initial(mt), &protocol, |_| Ok(()))
            .await
            .expect_err("malformed frame");
        assert!(format!("{err:?}").contains("failed to read the initial-request frame"));
    }

    async fn collect_frames(body: BoxBody) -> Vec<Message> {
        use http_body_util::BodyExt;
        let mut bytes = body.collect().await.expect("collects").to_bytes();
        let mut frames = Vec::new();
        while !bytes.is_empty() {
            frames.push(
                aws_smithy_eventstream::frame::read_message_from(&mut bytes).expect("valid frame"),
            );
        }
        frames
    }

    fn response_body_for(
        protocol: &SharedServerProtocol,
        initial_response: InitialResponsePolicy,
    ) -> Result<BoxBody, SerdeError> {
        let events: EventStreamSender<TestEvents, TestEventsError> =
            futures_util::stream::iter([Ok(TestEvents::Text(TextEvent {
                value: Some("evt".to_owned()),
            }))])
            .into();
        // The non-stream output members: reuse a simple struct as the output shape.
        let output = PlainEvent {
            text: Some("topic".to_owned()),
        };
        event_stream_response_body(
            &PLAIN_EVENT_SCHEMA,
            &output,
            events,
            SchemaEventMarshaller::<TestEvents>::new(protocol.clone()),
            SchemaEventErrorMarshaller::<TestEventsError>::new(protocol.clone()),
            protocol,
            initial_response,
        )
    }

    #[tokio::test]
    async fn response_body_prepends_the_initial_response_frame_when_asked() {
        let body = response_body_for(&cbor_protocol(), InitialResponsePolicy::Send).expect("builds");
        let frames = collect_frames(body).await;
        assert_eq!(frames.len(), 2);
        assert_eq!(string_header(&frames[0], ":event-type"), Some("initial-response"));
        assert_eq!(string_header(&frames[0], ":content-type"), Some("application/cbor"));
        assert_eq!(string_header(&frames[1], ":event-type"), Some("text"));
    }

    #[tokio::test]
    async fn response_body_omits_the_initial_response_frame_when_disabled() {
        let body = response_body_for(&cbor_protocol(), InitialResponsePolicy::Omit).expect("builds");
        let frames = collect_frames(body).await;
        assert_eq!(frames.len(), 1);
        assert_eq!(string_header(&frames[0], ":event-type"), Some("text"));
    }

    #[tokio::test]
    async fn response_body_omits_the_initial_response_frame_when_the_protocol_does_not_frame_it() {
        let body = response_body_for(&json_protocol(), InitialResponsePolicy::Send).expect("builds");
        let frames = collect_frames(body).await;
        assert_eq!(frames.len(), 1);
        assert_eq!(string_header(&frames[0], ":event-type"), Some("text"));
    }

    #[tokio::test]
    async fn response_body_carries_modeled_errors_as_exception_frames() {
        let protocol = json_protocol();
        let events: EventStreamSender<TestEvents, TestEventsError> =
            futures_util::stream::iter([Err(TestEventsError::Boom(BoomError {
                message: Some("failure".to_owned()),
            }))])
            .into();
        let output = PlainEvent { text: None };
        let body = event_stream_response_body(
            &PLAIN_EVENT_SCHEMA,
            &output,
            events,
            SchemaEventMarshaller::<TestEvents>::new(protocol.clone()),
            SchemaEventErrorMarshaller::<TestEventsError>::new(protocol.clone()),
            &protocol,
            InitialResponsePolicy::Omit,
        )
        .expect("builds");
        let frames = collect_frames(body).await;
        assert_eq!(frames.len(), 1);
        assert_eq!(string_header(&frames[0], ":message-type"), Some("exception"));
        assert_eq!(string_header(&frames[0], ":exception-type"), Some("boom"));
    }
}
