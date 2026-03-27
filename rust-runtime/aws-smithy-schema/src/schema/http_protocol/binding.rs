/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP binding protocol for REST-style APIs.

use crate::codec::{Codec, FinishSerializer};
use crate::protocol::ClientProtocol;
use crate::serde::{SerdeError, SerializableStruct, ShapeDeserializer, ShapeSerializer};
use crate::{Schema, ShapeId};
use aws_smithy_runtime_api::http::{Request, Response};
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::config_bag::ConfigBag;

/// An HTTP protocol for REST-style APIs that use HTTP bindings.
///
/// This protocol splits input members between HTTP locations (headers, query
/// strings, URI labels) and the payload based on HTTP binding traits
/// (`@httpHeader`, `@httpQuery`, `@httpLabel`, `@httpPayload`, etc.).
/// Non-bound members are serialized into the body using the provided codec.
///
/// # Type parameters
///
/// * `C` — the payload codec (e.g., `JsonCodec`, `XmlCodec`)
#[derive(Debug)]
pub struct HttpBindingProtocol<C> {
    protocol_id: ShapeId,
    codec: C,
    content_type: &'static str,
}

impl<C: Codec> HttpBindingProtocol<C> {
    /// Creates a new HTTP binding protocol.
    pub fn new(protocol_id: ShapeId, codec: C, content_type: &'static str) -> Self {
        Self {
            protocol_id,
            codec,
            content_type,
        }
    }
}

// Note: there is a percent_encoding crate we use some other places for this, but I'm trying to keep
// the dependencies to a minimum.
/// Percent-encode a string per RFC 3986 section 2.3 (unreserved characters only).
pub(crate) fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push(char::from(HEX[(byte >> 4) as usize]));
                out.push(char::from(HEX[(byte & 0x0f) as usize]));
            }
        }
    }
    out
}

pub(crate) const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// A ShapeSerializer that intercepts member writes and routes HTTP-bound
/// members to headers, query params, or URI labels instead of the body.
///
/// Members without HTTP binding traits are forwarded to the inner body
/// serializer unchanged.
struct HttpBindingSerializer<'a, S> {
    body: S,
    headers: Vec<(String, String)>,
    query_params: Vec<(String, String)>,
    labels: Vec<(String, String)>,
    /// When set, member schemas are resolved from this schema by name to find
    /// HTTP binding traits. This allows the protocol to override bindings
    /// (e.g., for presigning where body members become query params).
    input_schema: Option<&'a Schema>,
    /// True for the top-level input struct in serialize_request.
    /// Cleared after the first write_struct so nested structs delegate directly.
    is_top_level: bool,
    /// Tracks whether any member was written to the body serializer (i.e., a member
    /// without an HTTP binding trait). Used by `HttpBindingProtocol` to determine
    /// whether to wrap the body in `{}` and set `Content-Type: application/json`.
    /// Per the REST-JSON spec, operations with no body members must send an empty
    /// body with no Content-Type header.
    has_body_content: bool,
}

impl<'a, S> HttpBindingSerializer<'a, S> {
    fn new(body: S, input_schema: Option<&'a Schema>) -> Self {
        Self {
            body,
            headers: Vec::new(),
            query_params: Vec::new(),
            labels: Vec::new(),
            input_schema,
            is_top_level: true,
            has_body_content: false,
        }
    }

    /// Resolve the effective member schema: if an input_schema override is set,
    /// look up the member by name there (to get the correct HTTP bindings).
    /// Otherwise use the schema as-is.
    fn resolve_member<'s>(&self, schema: &'s Schema) -> &'s Schema
    where
        'a: 's,
    {
        if let (Some(input_schema), Some(name)) = (self.input_schema, schema.member_name()) {
            input_schema.member_schema(name).unwrap_or(schema)
        } else {
            schema
        }
    }
}

/// Helper: format a value as a string for HTTP binding serialization.
fn value_to_string<T: std::fmt::Display>(value: T) -> String {
    value.to_string()
}

impl<'a, S: ShapeSerializer> ShapeSerializer for HttpBindingSerializer<'a, S> {
    fn write_struct(
        &mut self,
        schema: &Schema,
        value: &dyn SerializableStruct,
    ) -> Result<(), SerdeError> {
        if self.is_top_level {
            // Top-level input struct: route serialize_members through the binder
            // so HTTP-bound members are intercepted. The body serializer's
            // write_struct is used for framing (e.g., { } for JSON), with a
            // proxy whose serialize_members delegates back to the binder.
            struct Proxy<'a, 'b, S> {
                binder: &'a mut HttpBindingSerializer<'b, S>,
                value: &'a dyn SerializableStruct,
            }
            impl<S: ShapeSerializer> SerializableStruct for Proxy<'_, '_, S> {
                fn serialize_members(
                    &self,
                    _serializer: &mut dyn ShapeSerializer,
                ) -> Result<(), SerdeError> {
                    let binder = self.binder as *const HttpBindingSerializer<'_, S>
                        as *mut HttpBindingSerializer<'_, S>;
                    // SAFETY: The body serializer called serialize_members on
                    // this proxy, passing &mut self (body). The binder wraps
                    // that same body serializer. We need mutable access to the
                    // binder to route writes. This is safe because:
                    // 1. The body serializer's write_struct only calls
                    //    serialize_members once, synchronously.
                    // 2. Body member writes from the binder go back to the
                    //    body serializer, which is in a valid state (between
                    //    the { and } it emitted).
                    self.value.serialize_members(unsafe { &mut *binder })
                }
            }
            // Clear is_top_level so nested write_struct calls (from body members)
            // take the else branch and delegate directly to the body serializer.
            // input_schema is preserved so resolve_member continues to work.
            self.is_top_level = false;
            let result = {
                let proxy = Proxy {
                    binder: self,
                    value,
                };
                let binder_ptr = &mut *proxy.binder as *mut HttpBindingSerializer<'_, S>;
                unsafe { (*binder_ptr).body.write_struct(schema, &proxy) }
            };
            result
        } else {
            // Nested struct (a body member targeting a structure): delegate
            // entirely to the body serializer.
            self.has_body_content = true;
            self.body.write_struct(schema, value)
        }
    }

    fn write_list(
        &mut self,
        schema: &Schema,
        write_elements: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.has_body_content = true;
        self.body.write_list(schema, write_elements)
    }

    fn write_map(
        &mut self,
        schema: &Schema,
        write_entries: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let schema = self.resolve_member(schema);
        // @httpPrefixHeaders: serialize map entries as prefixed headers
        if let Some(prefix) = schema.http_prefix_headers() {
            // Collect entries via a temporary serializer
            let mut collector = MapEntryCollector::new(prefix.value().to_string());
            write_entries(&mut collector)?;
            self.headers.extend(collector.entries);
            return Ok(());
        }
        // @httpQueryParams: serialize map entries as query params
        if schema.http_query_params().is_some() {
            let mut collector = MapEntryCollector::new(String::new());
            write_entries(&mut collector)?;
            for (k, v) in collector.entries {
                self.query_params.push((k, v));
            }
            return Ok(());
        }
        self.has_body_content = true;
        self.body.write_map(schema, write_entries)
    }

    fn write_boolean(&mut self, schema: &Schema, value: bool) -> Result<(), SerdeError> {
        let schema = self.resolve_member(schema);
        if let Some(binding) = http_string_binding(schema) {
            return self.add_binding(binding, schema, &value_to_string(value));
        }
        self.has_body_content = true;
        self.body.write_boolean(schema, value)
    }

    fn write_byte(&mut self, schema: &Schema, value: i8) -> Result<(), SerdeError> {
        let schema = self.resolve_member(schema);
        if let Some(binding) = http_string_binding(schema) {
            return self.add_binding(binding, schema, &value_to_string(value));
        }
        self.has_body_content = true;
        self.body.write_byte(schema, value)
    }

    fn write_short(&mut self, schema: &Schema, value: i16) -> Result<(), SerdeError> {
        let schema = self.resolve_member(schema);
        if let Some(binding) = http_string_binding(schema) {
            return self.add_binding(binding, schema, &value_to_string(value));
        }
        self.has_body_content = true;
        self.body.write_short(schema, value)
    }

    fn write_integer(&mut self, schema: &Schema, value: i32) -> Result<(), SerdeError> {
        let schema = self.resolve_member(schema);
        if let Some(binding) = http_string_binding(schema) {
            return self.add_binding(binding, schema, &value_to_string(value));
        }
        self.has_body_content = true;
        self.body.write_integer(schema, value)
    }

    fn write_long(&mut self, schema: &Schema, value: i64) -> Result<(), SerdeError> {
        let schema = self.resolve_member(schema);
        if let Some(binding) = http_string_binding(schema) {
            return self.add_binding(binding, schema, &value_to_string(value));
        }
        self.has_body_content = true;
        self.body.write_long(schema, value)
    }

    fn write_float(&mut self, schema: &Schema, value: f32) -> Result<(), SerdeError> {
        let schema = self.resolve_member(schema);
        if let Some(binding) = http_string_binding(schema) {
            return self.add_binding(binding, schema, &value_to_string(value));
        }
        self.has_body_content = true;
        self.body.write_float(schema, value)
    }

    fn write_double(&mut self, schema: &Schema, value: f64) -> Result<(), SerdeError> {
        let schema = self.resolve_member(schema);
        if let Some(binding) = http_string_binding(schema) {
            return self.add_binding(binding, schema, &value_to_string(value));
        }
        self.has_body_content = true;
        self.body.write_double(schema, value)
    }

    fn write_big_integer(
        &mut self,
        schema: &Schema,
        value: &aws_smithy_types::BigInteger,
    ) -> Result<(), SerdeError> {
        let schema = self.resolve_member(schema);
        if let Some(binding) = http_string_binding(schema) {
            return self.add_binding(binding, schema, value.as_ref());
        }
        self.has_body_content = true;
        self.body.write_big_integer(schema, value)
    }

    fn write_big_decimal(
        &mut self,
        schema: &Schema,
        value: &aws_smithy_types::BigDecimal,
    ) -> Result<(), SerdeError> {
        let schema = self.resolve_member(schema);
        if let Some(binding) = http_string_binding(schema) {
            return self.add_binding(binding, schema, value.as_ref());
        }
        self.has_body_content = true;
        self.body.write_big_decimal(schema, value)
    }

    fn write_string(&mut self, schema: &Schema, value: &str) -> Result<(), SerdeError> {
        let schema = self.resolve_member(schema);
        if let Some(binding) = http_string_binding(schema) {
            return self.add_binding(binding, schema, value);
        }
        self.has_body_content = true;
        self.body.write_string(schema, value)
    }

    fn write_blob(
        &mut self,
        schema: &Schema,
        value: &aws_smithy_types::Blob,
    ) -> Result<(), SerdeError> {
        let schema = self.resolve_member(schema);
        if schema.http_header().is_some() {
            let encoded = aws_smithy_types::base64::encode(value.as_ref());
            self.headers
                .push((schema.http_header().unwrap().value().to_string(), encoded));
            return Ok(());
        }
        self.has_body_content = true;
        self.body.write_blob(schema, value)
    }

    fn write_timestamp(
        &mut self,
        schema: &Schema,
        value: &aws_smithy_types::DateTime,
    ) -> Result<(), SerdeError> {
        let schema = self.resolve_member(schema);
        if let Some(binding) = http_string_binding(schema) {
            // Headers default to http-date, query/label default to date-time
            let format = if schema.timestamp_format().is_some() {
                match schema.timestamp_format().unwrap().format() {
                    crate::traits::TimestampFormat::EpochSeconds => {
                        aws_smithy_types::date_time::Format::EpochSeconds
                    }
                    crate::traits::TimestampFormat::HttpDate => {
                        aws_smithy_types::date_time::Format::HttpDate
                    }
                    crate::traits::TimestampFormat::DateTime => {
                        aws_smithy_types::date_time::Format::DateTime
                    }
                }
            } else {
                match binding {
                    HttpBinding::Header(_) => aws_smithy_types::date_time::Format::HttpDate,
                    _ => aws_smithy_types::date_time::Format::DateTime,
                }
            };
            let formatted = value
                .fmt(format)
                .map_err(|e| SerdeError::custom(format!("failed to format timestamp: {e}")))?;
            return self.add_binding(binding, schema, &formatted);
        }
        self.has_body_content = true;
        self.body.write_timestamp(schema, value)
    }

    fn write_document(
        &mut self,
        schema: &Schema,
        value: &aws_smithy_types::Document,
    ) -> Result<(), SerdeError> {
        self.has_body_content = true;
        self.body.write_document(schema, value)
    }

    fn write_null(&mut self, schema: &Schema) -> Result<(), SerdeError> {
        self.has_body_content = true;
        self.body.write_null(schema)
    }
}

/// Which HTTP location a member is bound to.
enum HttpBinding<'a> {
    Header(&'a str),
    Query(&'a str),
    Label,
}

/// Determine the HTTP binding for a member schema, if any.
fn http_string_binding(schema: &Schema) -> Option<HttpBinding<'_>> {
    if let Some(h) = schema.http_header() {
        return Some(HttpBinding::Header(h.value()));
    }
    if let Some(q) = schema.http_query() {
        return Some(HttpBinding::Query(q.value()));
    }
    if schema.http_label().is_some() {
        return Some(HttpBinding::Label);
    }
    None
}

impl<'a, S> HttpBindingSerializer<'a, S> {
    fn add_binding(
        &mut self,
        binding: HttpBinding<'_>,
        schema: &Schema,
        value: &str,
    ) -> Result<(), SerdeError> {
        match binding {
            HttpBinding::Header(name) => {
                self.headers.push((name.to_string(), value.to_string()));
            }
            HttpBinding::Query(name) => {
                self.query_params
                    .push((name.to_string(), value.to_string()));
            }
            HttpBinding::Label => {
                let name = schema
                    .member_name()
                    .ok_or_else(|| SerdeError::custom("httpLabel on non-member schema"))?;
                self.labels.push((name.to_string(), value.to_string()));
            }
        }
        Ok(())
    }
}

/// Collects map key-value pairs written via ShapeSerializer for
/// @httpPrefixHeaders and @httpQueryParams.
struct MapEntryCollector {
    prefix: String,
    entries: Vec<(String, String)>,
    pending_key: Option<String>,
}

impl MapEntryCollector {
    fn new(prefix: String) -> Self {
        Self {
            prefix,
            entries: Vec::new(),
            pending_key: None,
        }
    }
}

impl ShapeSerializer for MapEntryCollector {
    fn write_string(&mut self, _schema: &Schema, value: &str) -> Result<(), SerdeError> {
        if let Some(key) = self.pending_key.take() {
            self.entries
                .push((format!("{}{}", self.prefix, key), value.to_string()));
        } else {
            self.pending_key = Some(value.to_string());
        }
        Ok(())
    }

    // All other methods are no-ops — maps in HTTP bindings only have string keys/values.
    fn write_struct(&mut self, _: &Schema, _: &dyn SerializableStruct) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_list(
        &mut self,
        _: &Schema,
        _: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_map(
        &mut self,
        _: &Schema,
        _: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_boolean(&mut self, _: &Schema, _: bool) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_byte(&mut self, _: &Schema, _: i8) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_short(&mut self, _: &Schema, _: i16) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_integer(&mut self, _: &Schema, _: i32) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_long(&mut self, _: &Schema, _: i64) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_float(&mut self, _: &Schema, _: f32) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_double(&mut self, _: &Schema, _: f64) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_big_integer(
        &mut self,
        _: &Schema,
        _: &aws_smithy_types::BigInteger,
    ) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_big_decimal(
        &mut self,
        _: &Schema,
        _: &aws_smithy_types::BigDecimal,
    ) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_blob(&mut self, _: &Schema, _: &aws_smithy_types::Blob) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_timestamp(
        &mut self,
        _: &Schema,
        _: &aws_smithy_types::DateTime,
    ) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_document(
        &mut self,
        _: &Schema,
        _: &aws_smithy_types::Document,
    ) -> Result<(), SerdeError> {
        Ok(())
    }
    fn write_null(&mut self, _: &Schema) -> Result<(), SerdeError> {
        Ok(())
    }
}

/// A composite deserializer that reads HTTP-bound members from the response
/// headers/status and delegates body members to the inner codec deserializer.
///
/// Individual `read_*` methods check the member schema for HTTP binding traits
/// and read from the response headers/status when present, falling through to
/// the body deserializer otherwise.
///
/// `read_struct` iterates members: HTTP-bound members are read directly from
/// this deserializer, while remaining members are delegated to the body
/// deserializer via `body.read_struct`.
pub struct HttpBindingDeserializer<'a, D> {
    body: D,
    response: &'a Response,
}

impl<'a, D: ShapeDeserializer> HttpBindingDeserializer<'a, D> {
    fn read_header(&self, name: &str) -> Option<&str> {
        self.response.headers().get(name)
    }
}

impl<'a, D: ShapeDeserializer> ShapeDeserializer for HttpBindingDeserializer<'a, D> {
    fn read_struct(
        &mut self,
        schema: &Schema,
        consumer: &mut dyn FnMut(&Schema, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        // Check for @httpPayload — the entire body is a single member's value.
        for member in schema.members() {
            if member.http_payload().is_some() {
                consumer(member, &mut self.body)?;
                return Ok(());
            }
        }
        // No @httpPayload — body members are in the protocol-specific document.
        // Header-bound members are handled by generated code in
        // `deserialize_nonstreaming` which reads them directly from the HTTP
        // response before calling this deserializer. This avoids the overhead
        // of iterating all schema members and checking HTTP binding traits at
        // runtime.
        self.body.read_struct(schema, consumer)
    }

    fn read_list(
        &mut self,
        schema: &Schema,
        consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.body.read_list(schema, consumer)
    }

    fn read_map(
        &mut self,
        schema: &Schema,
        consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.body.read_map(schema, consumer)
    }

    fn read_boolean(&mut self, schema: &Schema) -> Result<bool, SerdeError> {
        if let Some(h) = schema.http_header() {
            let val = self
                .read_header(h.value())
                .ok_or_else(|| SerdeError::MissingMember {
                    member_name: h.value().to_string(),
                })?;
            return val.parse().map_err(|_| SerdeError::InvalidInput {
                message: format!("invalid boolean header: {val}"),
            });
        }
        self.body.read_boolean(schema)
    }

    fn read_byte(&mut self, schema: &Schema) -> Result<i8, SerdeError> {
        if let Some(h) = schema.http_header() {
            let val = self
                .read_header(h.value())
                .ok_or_else(|| SerdeError::MissingMember {
                    member_name: h.value().to_string(),
                })?;
            return val.parse().map_err(|_| SerdeError::InvalidInput {
                message: format!("invalid byte header: {val}"),
            });
        }
        self.body.read_byte(schema)
    }

    fn read_short(&mut self, schema: &Schema) -> Result<i16, SerdeError> {
        if let Some(h) = schema.http_header() {
            let val = self
                .read_header(h.value())
                .ok_or_else(|| SerdeError::MissingMember {
                    member_name: h.value().to_string(),
                })?;
            return val.parse().map_err(|_| SerdeError::InvalidInput {
                message: format!("invalid short header: {val}"),
            });
        }
        self.body.read_short(schema)
    }

    fn read_integer(&mut self, schema: &Schema) -> Result<i32, SerdeError> {
        if schema.http_response_code().is_some() {
            return Ok(self.response.status().as_u16() as i32);
        }
        if let Some(h) = schema.http_header() {
            let val = self
                .read_header(h.value())
                .ok_or_else(|| SerdeError::MissingMember {
                    member_name: h.value().to_string(),
                })?;
            return val.parse().map_err(|_| SerdeError::InvalidInput {
                message: format!("invalid integer header: {val}"),
            });
        }
        self.body.read_integer(schema)
    }

    fn read_long(&mut self, schema: &Schema) -> Result<i64, SerdeError> {
        if let Some(h) = schema.http_header() {
            let val = self
                .read_header(h.value())
                .ok_or_else(|| SerdeError::MissingMember {
                    member_name: h.value().to_string(),
                })?;
            return val.parse().map_err(|_| SerdeError::InvalidInput {
                message: format!("invalid long header: {val}"),
            });
        }
        self.body.read_long(schema)
    }

    fn read_float(&mut self, schema: &Schema) -> Result<f32, SerdeError> {
        if let Some(h) = schema.http_header() {
            let val = self
                .read_header(h.value())
                .ok_or_else(|| SerdeError::MissingMember {
                    member_name: h.value().to_string(),
                })?;
            return val.parse().map_err(|_| SerdeError::InvalidInput {
                message: format!("invalid float header: {val}"),
            });
        }
        self.body.read_float(schema)
    }

    fn read_double(&mut self, schema: &Schema) -> Result<f64, SerdeError> {
        if let Some(h) = schema.http_header() {
            let val = self
                .read_header(h.value())
                .ok_or_else(|| SerdeError::MissingMember {
                    member_name: h.value().to_string(),
                })?;
            return val.parse().map_err(|_| SerdeError::InvalidInput {
                message: format!("invalid double header: {val}"),
            });
        }
        self.body.read_double(schema)
    }

    fn read_big_integer(
        &mut self,
        schema: &Schema,
    ) -> Result<aws_smithy_types::BigInteger, SerdeError> {
        self.body.read_big_integer(schema)
    }

    fn read_big_decimal(
        &mut self,
        schema: &Schema,
    ) -> Result<aws_smithy_types::BigDecimal, SerdeError> {
        self.body.read_big_decimal(schema)
    }

    fn read_string(&mut self, schema: &Schema) -> Result<String, SerdeError> {
        if let Some(h) = schema.http_header() {
            return self
                .read_header(h.value())
                .map(|v| v.to_string())
                .ok_or_else(|| SerdeError::MissingMember {
                    member_name: h.value().to_string(),
                });
        }
        self.body.read_string(schema)
    }

    fn read_blob(&mut self, schema: &Schema) -> Result<aws_smithy_types::Blob, SerdeError> {
        self.body.read_blob(schema)
    }

    fn read_timestamp(
        &mut self,
        schema: &Schema,
    ) -> Result<aws_smithy_types::DateTime, SerdeError> {
        if let Some(h) = schema.http_header() {
            let val = self
                .read_header(h.value())
                .ok_or_else(|| SerdeError::MissingMember {
                    member_name: h.value().to_string(),
                })?;
            return aws_smithy_types::DateTime::from_str(
                val,
                aws_smithy_types::date_time::Format::HttpDate,
            )
            .map_err(|e| SerdeError::InvalidInput {
                message: format!("invalid timestamp header: {e}"),
            });
        }
        self.body.read_timestamp(schema)
    }

    fn read_document(&mut self, schema: &Schema) -> Result<aws_smithy_types::Document, SerdeError> {
        self.body.read_document(schema)
    }

    fn is_null(&self) -> bool {
        self.body.is_null()
    }

    fn container_size(&self) -> Option<usize> {
        self.body.container_size()
    }
}

impl<C> ClientProtocol for HttpBindingProtocol<C>
where
    C: Codec + Send + Sync + std::fmt::Debug + 'static,
    for<'a> C::Deserializer<'a>: ShapeDeserializer,
{
    fn protocol_id(&self) -> &ShapeId {
        &self.protocol_id
    }

    fn serialize_request(
        &self,
        input: &dyn SerializableStruct,
        input_schema: &Schema,
        endpoint: &str,
        _cfg: &ConfigBag,
    ) -> Result<Request, SerdeError> {
        let mut binder =
            HttpBindingSerializer::new(self.codec.create_serializer(), Some(input_schema));

        // Check if there's an @httpPayload member targeting a structure/union.
        // In that case, the payload member's own write_struct provides the body
        // framing, so we must not add top-level struct framing.
        let has_struct_payload = input_schema.members().iter().any(|m| {
            m.http_payload().is_some()
                && matches!(
                    m.shape_type(),
                    crate::ShapeType::Structure | crate::ShapeType::Union
                )
        });
        if has_struct_payload {
            input.serialize_members(&mut binder)?;
        } else {
            binder.write_struct(input_schema, input)?;
        }
        let mut body = binder.body.finish();

        // Per the REST-JSON content-type handling spec:
        // - If @httpPayload targets a blob/string: send raw bytes, no Content-Type when empty
        // - If body members exist (even if all optional and unset): send `{}` with Content-Type
        // - If no body members at all (everything is in headers/query/labels): empty body, no Content-Type
        let has_blob_or_string_payload = input_schema.members().iter().any(|m| {
            m.http_payload().is_some()
                && !matches!(
                    m.shape_type(),
                    crate::ShapeType::Structure | crate::ShapeType::Union
                )
        });
        let has_body_members = input_schema.members().iter().any(|m| {
            m.http_header().is_none()
                && m.http_query().is_none()
                && m.http_label().is_none()
                && m.http_prefix_headers().is_none()
                && m.http_query_params().is_none()
        });

        let set_content_type = if has_blob_or_string_payload {
            // Blob/string payload: only set Content-Type if there's actual content
            !body.is_empty()
        } else if has_body_members {
            // Operation has body members — body includes framing (e.g., `{}`).
            // Per the REST-JSON spec, even if all members are optional and unset, send `{}`.
            true
        } else {
            // No body members at all — empty body, no Content-Type.
            body = Vec::new();
            false
        };

        // Build URI: use @http trait if available (with label substitution from binder),
        // otherwise fall back to endpoint with manual label substitution.
        let mut uri = match input_schema.http() {
            Some(h) => {
                let mut path = h.uri().to_string();
                for (name, value) in &binder.labels {
                    let placeholder = format!("{{{name}}}");
                    path = path.replace(&placeholder, &percent_encode(value));
                }
                if endpoint.is_empty() {
                    path
                } else {
                    format!("{}{}", endpoint, path)
                }
            }
            None => {
                let mut u = if endpoint.is_empty() {
                    "/".to_string()
                } else {
                    endpoint.to_string()
                };
                for (name, value) in &binder.labels {
                    let placeholder = format!("{{{name}}}");
                    u = u.replace(&placeholder, &percent_encode(value));
                }
                u
            }
        };
        if !binder.query_params.is_empty() {
            uri.push(if uri.contains('?') { '&' } else { '?' });
            let pairs: Vec<String> = binder
                .query_params
                .iter()
                .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
                .collect();
            uri.push_str(&pairs.join("&"));
        }

        let mut request = Request::new(SdkBody::from(body));
        // Set HTTP method from @http trait
        if let Some(http) = input_schema.http() {
            request
                .set_method(http.method())
                .map_err(|e| SerdeError::custom(format!("invalid HTTP method: {e}")))?;
        }
        request
            .set_uri(uri.as_str())
            .map_err(|e| SerdeError::custom(format!("invalid endpoint URI: {e}")))?;
        if set_content_type {
            request
                .headers_mut()
                .insert("Content-Type", self.content_type);
        }
        if let Some(len) = request.body().content_length() {
            if len > 0 || set_content_type {
                request
                    .headers_mut()
                    .insert("Content-Length", len.to_string());
            }
        }
        for (name, value) in &binder.headers {
            request.headers_mut().insert(name.clone(), value.clone());
        }
        Ok(request)
    }

    fn deserialize_response<'a>(
        &self,
        response: &'a Response,
        _output_schema: &Schema,
        _cfg: &ConfigBag,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError> {
        let body = response
            .body()
            .bytes()
            .ok_or_else(|| SerdeError::custom("response body is not available as bytes"))?;
        Ok(Box::new(HttpBindingDeserializer {
            body: self.codec.create_deserializer(body),
            response,
        }))
    }

    fn deserialize_body<'a>(
        &self,
        body: &'a [u8],
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, SerdeError> {
        Ok(Box::new(self.codec.create_deserializer(body)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serde::SerializableStruct;
    use crate::{prelude::*, ShapeType};

    struct TestSerializer {
        output: Vec<u8>,
    }

    impl FinishSerializer for TestSerializer {
        fn finish(self) -> Vec<u8> {
            self.output
        }
    }

    impl ShapeSerializer for TestSerializer {
        fn write_struct(
            &mut self,
            _: &Schema,
            value: &dyn SerializableStruct,
        ) -> Result<(), SerdeError> {
            self.output.push(b'{');
            value.serialize_members(self)?;
            self.output.push(b'}');
            Ok(())
        }
        fn write_list(
            &mut self,
            _: &Schema,
            _: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
        ) -> Result<(), SerdeError> {
            Ok(())
        }
        fn write_map(
            &mut self,
            _: &Schema,
            _: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
        ) -> Result<(), SerdeError> {
            Ok(())
        }
        fn write_boolean(&mut self, _: &Schema, _: bool) -> Result<(), SerdeError> {
            Ok(())
        }
        fn write_byte(&mut self, _: &Schema, _: i8) -> Result<(), SerdeError> {
            Ok(())
        }
        fn write_short(&mut self, _: &Schema, _: i16) -> Result<(), SerdeError> {
            Ok(())
        }
        fn write_integer(&mut self, _: &Schema, _: i32) -> Result<(), SerdeError> {
            Ok(())
        }
        fn write_long(&mut self, _: &Schema, _: i64) -> Result<(), SerdeError> {
            Ok(())
        }
        fn write_float(&mut self, _: &Schema, _: f32) -> Result<(), SerdeError> {
            Ok(())
        }
        fn write_double(&mut self, _: &Schema, _: f64) -> Result<(), SerdeError> {
            Ok(())
        }
        fn write_big_integer(
            &mut self,
            _: &Schema,
            _: &aws_smithy_types::BigInteger,
        ) -> Result<(), SerdeError> {
            Ok(())
        }
        fn write_big_decimal(
            &mut self,
            _: &Schema,
            _: &aws_smithy_types::BigDecimal,
        ) -> Result<(), SerdeError> {
            Ok(())
        }
        fn write_string(&mut self, _: &Schema, v: &str) -> Result<(), SerdeError> {
            self.output.extend_from_slice(v.as_bytes());
            Ok(())
        }
        fn write_blob(&mut self, _: &Schema, _: &aws_smithy_types::Blob) -> Result<(), SerdeError> {
            Ok(())
        }
        fn write_timestamp(
            &mut self,
            _: &Schema,
            _: &aws_smithy_types::DateTime,
        ) -> Result<(), SerdeError> {
            Ok(())
        }
        fn write_document(
            &mut self,
            _: &Schema,
            _: &aws_smithy_types::Document,
        ) -> Result<(), SerdeError> {
            Ok(())
        }
        fn write_null(&mut self, _: &Schema) -> Result<(), SerdeError> {
            Ok(())
        }
    }

    struct TestDeserializer<'a> {
        input: &'a [u8],
    }

    impl ShapeDeserializer for TestDeserializer<'_> {
        fn read_struct(
            &mut self,
            _: &Schema,
            _: &mut dyn FnMut(&Schema, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
        ) -> Result<(), SerdeError> {
            Ok(())
        }
        fn read_list(
            &mut self,
            _: &Schema,
            _: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
        ) -> Result<(), SerdeError> {
            Ok(())
        }
        fn read_map(
            &mut self,
            _: &Schema,
            _: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
        ) -> Result<(), SerdeError> {
            Ok(())
        }
        fn read_boolean(&mut self, _: &Schema) -> Result<bool, SerdeError> {
            Ok(false)
        }
        fn read_byte(&mut self, _: &Schema) -> Result<i8, SerdeError> {
            Ok(0)
        }
        fn read_short(&mut self, _: &Schema) -> Result<i16, SerdeError> {
            Ok(0)
        }
        fn read_integer(&mut self, _: &Schema) -> Result<i32, SerdeError> {
            Ok(0)
        }
        fn read_long(&mut self, _: &Schema) -> Result<i64, SerdeError> {
            Ok(0)
        }
        fn read_float(&mut self, _: &Schema) -> Result<f32, SerdeError> {
            Ok(0.0)
        }
        fn read_double(&mut self, _: &Schema) -> Result<f64, SerdeError> {
            Ok(0.0)
        }
        fn read_big_integer(
            &mut self,
            _: &Schema,
        ) -> Result<aws_smithy_types::BigInteger, SerdeError> {
            use std::str::FromStr;
            Ok(aws_smithy_types::BigInteger::from_str("0").unwrap())
        }
        fn read_big_decimal(
            &mut self,
            _: &Schema,
        ) -> Result<aws_smithy_types::BigDecimal, SerdeError> {
            use std::str::FromStr;
            Ok(aws_smithy_types::BigDecimal::from_str("0").unwrap())
        }
        fn read_string(&mut self, _: &Schema) -> Result<String, SerdeError> {
            Ok(String::from_utf8_lossy(self.input).into_owned())
        }
        fn read_blob(&mut self, _: &Schema) -> Result<aws_smithy_types::Blob, SerdeError> {
            Ok(aws_smithy_types::Blob::new(vec![]))
        }
        fn read_timestamp(&mut self, _: &Schema) -> Result<aws_smithy_types::DateTime, SerdeError> {
            Ok(aws_smithy_types::DateTime::from_secs(0))
        }
        fn read_document(&mut self, _: &Schema) -> Result<aws_smithy_types::Document, SerdeError> {
            Ok(aws_smithy_types::Document::Null)
        }
        fn is_null(&self) -> bool {
            false
        }
        fn container_size(&self) -> Option<usize> {
            None
        }
    }

    #[derive(Debug)]
    struct TestCodec;

    impl Codec for TestCodec {
        type Serializer = TestSerializer;
        type Deserializer<'a> = TestDeserializer<'a>;
        fn create_serializer(&self) -> Self::Serializer {
            TestSerializer { output: Vec::new() }
        }
        fn create_deserializer<'a>(&self, input: &'a [u8]) -> Self::Deserializer<'a> {
            TestDeserializer { input }
        }
    }

    static TEST_SCHEMA: Schema =
        Schema::new(crate::shape_id!("test", "TestStruct"), ShapeType::Structure);

    struct EmptyStruct;
    impl SerializableStruct for EmptyStruct {
        fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            Ok(())
        }
    }

    static NAME_MEMBER: Schema = Schema::new_member(
        crate::shape_id!("test", "TestStruct"),
        ShapeType::String,
        "name",
        0,
    );
    static MEMBERS: &[&Schema] = &[&NAME_MEMBER];
    static STRUCT_WITH_MEMBER: Schema = Schema::new_struct(
        crate::shape_id!("test", "TestStruct"),
        ShapeType::Structure,
        MEMBERS,
    );

    struct NameStruct;
    impl SerializableStruct for NameStruct {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_string(&NAME_MEMBER, "Alice")
        }
    }

    fn make_protocol() -> HttpBindingProtocol<TestCodec> {
        HttpBindingProtocol::new(
            crate::shape_id!("test", "proto"),
            TestCodec,
            "application/test",
        )
    }

    #[test]
    fn serialize_sets_content_type() {
        // A struct with body members gets Content-Type
        let request = make_protocol()
            .serialize_request(
                &EmptyStruct,
                &STRUCT_WITH_MEMBER,
                "https://example.com",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(
            request.headers().get("Content-Type").unwrap(),
            "application/test"
        );
    }

    #[test]
    fn serialize_no_body_members_omits_content_type() {
        // A struct with no members gets no Content-Type per REST-JSON spec
        let request = make_protocol()
            .serialize_request(
                &EmptyStruct,
                &TEST_SCHEMA,
                "https://example.com",
                &ConfigBag::base(),
            )
            .unwrap();
        assert!(request.headers().get("Content-Type").is_none());
    }

    #[test]
    fn serialize_sets_uri() {
        let request = make_protocol()
            .serialize_request(
                &EmptyStruct,
                &TEST_SCHEMA,
                "https://example.com/path",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(request.uri(), "https://example.com/path");
    }

    #[test]
    fn serialize_body() {
        let request = make_protocol()
            .serialize_request(
                &NameStruct,
                &STRUCT_WITH_MEMBER,
                "https://example.com",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(request.body().bytes().unwrap(), b"{Alice}");
    }

    #[test]
    fn deserialize_response() {
        let response = Response::new(
            200u16.try_into().unwrap(),
            SdkBody::from(r#"{"name":"Bob"}"#),
        );
        let mut deser = make_protocol()
            .deserialize_response(&response, &TEST_SCHEMA, &ConfigBag::base())
            .unwrap();
        assert_eq!(deser.read_string(&STRING).unwrap(), r#"{"name":"Bob"}"#);
    }

    #[test]
    fn update_endpoint() {
        let mut request = make_protocol()
            .serialize_request(
                &EmptyStruct,
                &TEST_SCHEMA,
                "https://old.example.com",
                &ConfigBag::base(),
            )
            .unwrap();
        let endpoint = aws_smithy_types::endpoint::Endpoint::builder()
            .url("https://new.example.com")
            .build();
        make_protocol()
            .update_endpoint(&mut request, &endpoint, &ConfigBag::base())
            .unwrap();
        assert_eq!(request.uri(), "https://new.example.com/");
    }

    #[test]
    fn protocol_id() {
        let protocol = HttpBindingProtocol::new(
            crate::shape_id!("aws.protocols", "restJson1"),
            TestCodec,
            "application/json",
        );
        assert_eq!(protocol.protocol_id().as_str(), "aws.protocols#restJson1");
    }

    #[test]
    fn invalid_uri_returns_error() {
        assert!(make_protocol()
            .serialize_request(
                &EmptyStruct,
                &TEST_SCHEMA,
                "not a valid uri\n\n",
                &ConfigBag::base()
            )
            .is_err());
    }

    // -- @httpHeader tests --

    static HEADER_MEMBER: Schema = Schema::new_member(
        crate::shape_id!("test", "S"),
        ShapeType::String,
        "xToken",
        0,
    )
    .with_http_header("X-Token");

    static HEADER_SCHEMA: Schema = Schema::new_struct(
        crate::shape_id!("test", "S"),
        ShapeType::Structure,
        &[&HEADER_MEMBER],
    );

    struct HeaderStruct;
    impl SerializableStruct for HeaderStruct {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_string(&HEADER_MEMBER, "my-token-value")
        }
    }

    #[test]
    fn http_header_string() {
        let request = make_protocol()
            .serialize_request(
                &HeaderStruct,
                &HEADER_SCHEMA,
                "https://example.com",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(request.headers().get("X-Token").unwrap(), "my-token-value");
    }

    static INT_HEADER_MEMBER: Schema = Schema::new_member(
        crate::shape_id!("test", "S"),
        ShapeType::Integer,
        "retryCount",
        0,
    )
    .with_http_header("X-Retry-Count");

    static INT_HEADER_SCHEMA: Schema = Schema::new_struct(
        crate::shape_id!("test", "S"),
        ShapeType::Structure,
        &[&INT_HEADER_MEMBER],
    );

    struct IntHeaderStruct;
    impl SerializableStruct for IntHeaderStruct {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_integer(&INT_HEADER_MEMBER, 3)
        }
    }

    #[test]
    fn http_header_integer() {
        let request = make_protocol()
            .serialize_request(
                &IntHeaderStruct,
                &INT_HEADER_SCHEMA,
                "https://example.com",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(request.headers().get("X-Retry-Count").unwrap(), "3");
    }

    static BOOL_HEADER_MEMBER: Schema = Schema::new_member(
        crate::shape_id!("test", "S"),
        ShapeType::Boolean,
        "verbose",
        0,
    )
    .with_http_header("X-Verbose");

    static BOOL_HEADER_SCHEMA: Schema = Schema::new_struct(
        crate::shape_id!("test", "S"),
        ShapeType::Structure,
        &[&BOOL_HEADER_MEMBER],
    );

    struct BoolHeaderStruct;
    impl SerializableStruct for BoolHeaderStruct {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_boolean(&BOOL_HEADER_MEMBER, true)
        }
    }

    #[test]
    fn http_header_boolean() {
        let request = make_protocol()
            .serialize_request(
                &BoolHeaderStruct,
                &BOOL_HEADER_SCHEMA,
                "https://example.com",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(request.headers().get("X-Verbose").unwrap(), "true");
    }

    // -- @httpQuery tests --

    static QUERY_MEMBER: Schema =
        Schema::new_member(crate::shape_id!("test", "S"), ShapeType::String, "color", 0)
            .with_http_query("color");

    static QUERY_SCHEMA: Schema = Schema::new_struct(
        crate::shape_id!("test", "S"),
        ShapeType::Structure,
        &[&QUERY_MEMBER],
    );

    struct QueryStruct;
    impl SerializableStruct for QueryStruct {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_string(&QUERY_MEMBER, "blue")
        }
    }

    #[test]
    fn http_query_string() {
        let request = make_protocol()
            .serialize_request(
                &QueryStruct,
                &QUERY_SCHEMA,
                "https://example.com/things",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(request.uri(), "https://example.com/things?color=blue");
    }

    static INT_QUERY_MEMBER: Schema =
        Schema::new_member(crate::shape_id!("test", "S"), ShapeType::Integer, "size", 0)
            .with_http_query("size");

    static INT_QUERY_SCHEMA: Schema = Schema::new_struct(
        crate::shape_id!("test", "S"),
        ShapeType::Structure,
        &[&INT_QUERY_MEMBER],
    );

    struct IntQueryStruct;
    impl SerializableStruct for IntQueryStruct {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_integer(&INT_QUERY_MEMBER, 42)
        }
    }

    #[test]
    fn http_query_integer() {
        let request = make_protocol()
            .serialize_request(
                &IntQueryStruct,
                &INT_QUERY_SCHEMA,
                "https://example.com/things",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(request.uri(), "https://example.com/things?size=42");
    }

    // -- Multiple @httpQuery params --

    static Q1: Schema =
        Schema::new_member(crate::shape_id!("test", "S"), ShapeType::String, "a", 0)
            .with_http_query("a");
    static Q2: Schema =
        Schema::new_member(crate::shape_id!("test", "S"), ShapeType::String, "b", 1)
            .with_http_query("b");
    static MULTI_QUERY_SCHEMA: Schema = Schema::new_struct(
        crate::shape_id!("test", "S"),
        ShapeType::Structure,
        &[&Q1, &Q2],
    );

    struct MultiQueryStruct;
    impl SerializableStruct for MultiQueryStruct {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_string(&Q1, "x")?;
            s.write_string(&Q2, "y")
        }
    }

    #[test]
    fn http_query_multiple_params() {
        let request = make_protocol()
            .serialize_request(
                &MultiQueryStruct,
                &MULTI_QUERY_SCHEMA,
                "https://example.com",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(request.uri(), "https://example.com?a=x&b=y");
    }

    // -- @httpQuery with percent-encoding --

    #[test]
    fn http_query_percent_encodes_values() {
        struct SpaceQueryStruct;
        impl SerializableStruct for SpaceQueryStruct {
            fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                s.write_string(&QUERY_MEMBER, "hello world")
            }
        }
        let request = make_protocol()
            .serialize_request(
                &SpaceQueryStruct,
                &QUERY_SCHEMA,
                "https://example.com",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(request.uri(), "https://example.com?color=hello%20world");
    }

    // -- @httpLabel tests --

    static LABEL_MEMBER: Schema = Schema::new_member(
        crate::shape_id!("test", "S"),
        ShapeType::String,
        "bucketName",
        0,
    )
    .with_http_label();

    static LABEL_SCHEMA: Schema = Schema::new_struct(
        crate::shape_id!("test", "S"),
        ShapeType::Structure,
        &[&LABEL_MEMBER],
    );

    struct LabelStruct;
    impl SerializableStruct for LabelStruct {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_string(&LABEL_MEMBER, "my-bucket")
        }
    }

    #[test]
    fn http_label_substitution() {
        let request = make_protocol()
            .serialize_request(
                &LabelStruct,
                &LABEL_SCHEMA,
                "https://example.com/{bucketName}/objects",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(request.uri(), "https://example.com/my-bucket/objects");
    }

    #[test]
    fn http_label_percent_encodes() {
        struct SpecialLabelStruct;
        impl SerializableStruct for SpecialLabelStruct {
            fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                s.write_string(&LABEL_MEMBER, "my bucket/name")
            }
        }
        let request = make_protocol()
            .serialize_request(
                &SpecialLabelStruct,
                &LABEL_SCHEMA,
                "https://example.com/{bucketName}",
                &ConfigBag::base(),
            )
            .unwrap();
        assert!(request.uri().contains("my%20bucket%2Fname"));
    }

    static INT_LABEL_MEMBER: Schema = Schema::new_member(
        crate::shape_id!("test", "S"),
        ShapeType::Integer,
        "itemId",
        0,
    )
    .with_http_label();

    static INT_LABEL_SCHEMA: Schema = Schema::new_struct(
        crate::shape_id!("test", "S"),
        ShapeType::Structure,
        &[&INT_LABEL_MEMBER],
    );

    struct IntLabelStruct;
    impl SerializableStruct for IntLabelStruct {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_integer(&INT_LABEL_MEMBER, 123)
        }
    }

    #[test]
    fn http_label_integer() {
        let request = make_protocol()
            .serialize_request(
                &IntLabelStruct,
                &INT_LABEL_SCHEMA,
                "https://example.com/items/{itemId}",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(request.uri(), "https://example.com/items/123");
    }

    // -- Combined: @httpHeader + @httpQuery + @httpLabel + body --

    static COMBINED_LABEL: Schema =
        Schema::new_member(crate::shape_id!("test", "S"), ShapeType::String, "id", 0)
            .with_http_label();
    static COMBINED_HEADER: Schema =
        Schema::new_member(crate::shape_id!("test", "S"), ShapeType::String, "token", 1)
            .with_http_header("X-Token");
    static COMBINED_QUERY: Schema = Schema::new_member(
        crate::shape_id!("test", "S"),
        ShapeType::String,
        "filter",
        2,
    )
    .with_http_query("filter");
    static COMBINED_BODY: Schema =
        Schema::new_member(crate::shape_id!("test", "S"), ShapeType::String, "data", 3);
    static COMBINED_SCHEMA: Schema = Schema::new_struct(
        crate::shape_id!("test", "S"),
        ShapeType::Structure,
        &[
            &COMBINED_LABEL,
            &COMBINED_HEADER,
            &COMBINED_QUERY,
            &COMBINED_BODY,
        ],
    );

    struct CombinedStruct;
    impl SerializableStruct for CombinedStruct {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_string(&COMBINED_LABEL, "item-42")?;
            s.write_string(&COMBINED_HEADER, "secret")?;
            s.write_string(&COMBINED_QUERY, "active")?;
            s.write_string(&COMBINED_BODY, "payload-data")
        }
    }

    #[test]
    fn combined_bindings() {
        let request = make_protocol()
            .serialize_request(
                &CombinedStruct,
                &COMBINED_SCHEMA,
                "https://example.com/{id}/details",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(
            request.uri(),
            "https://example.com/item-42/details?filter=active"
        );
        // Header
        assert_eq!(request.headers().get("X-Token").unwrap(), "secret");
        // Body contains only the unbound member
        let body = request.body().bytes().unwrap();
        assert!(body
            .windows(b"payload-data".len())
            .any(|w| w == b"payload-data"));
    }

    // -- @httpPrefixHeaders tests --

    static PREFIX_MEMBER: Schema =
        Schema::new_member(crate::shape_id!("test", "S"), ShapeType::Map, "metadata", 0)
            .with_http_prefix_headers("X-Meta-");

    static PREFIX_SCHEMA: Schema = Schema::new_struct(
        crate::shape_id!("test", "S"),
        ShapeType::Structure,
        &[&PREFIX_MEMBER],
    );

    struct PrefixHeaderStruct;
    impl SerializableStruct for PrefixHeaderStruct {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_map(&PREFIX_MEMBER, &|s| {
                s.write_string(&STRING, "Color")?;
                s.write_string(&STRING, "red")?;
                s.write_string(&STRING, "Size")?;
                s.write_string(&STRING, "large")?;
                Ok(())
            })
        }
    }

    #[test]
    fn http_prefix_headers() {
        let request = make_protocol()
            .serialize_request(
                &PrefixHeaderStruct,
                &PREFIX_SCHEMA,
                "https://example.com",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(request.headers().get("X-Meta-Color").unwrap(), "red");
        assert_eq!(request.headers().get("X-Meta-Size").unwrap(), "large");
    }

    // -- @httpQueryParams tests --

    static QUERY_PARAMS_MEMBER: Schema =
        Schema::new_member(crate::shape_id!("test", "S"), ShapeType::Map, "params", 0)
            .with_http_query_params();

    static QUERY_PARAMS_SCHEMA: Schema = Schema::new_struct(
        crate::shape_id!("test", "S"),
        ShapeType::Structure,
        &[&QUERY_PARAMS_MEMBER],
    );

    struct QueryParamsStruct;
    impl SerializableStruct for QueryParamsStruct {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_map(&QUERY_PARAMS_MEMBER, &|s| {
                s.write_string(&STRING, "page")?;
                s.write_string(&STRING, "2")?;
                s.write_string(&STRING, "limit")?;
                s.write_string(&STRING, "50")?;
                Ok(())
            })
        }
    }

    #[test]
    fn http_query_params() {
        let request = make_protocol()
            .serialize_request(
                &QueryParamsStruct,
                &QUERY_PARAMS_SCHEMA,
                "https://example.com",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(request.uri(), "https://example.com?page=2&limit=50");
    }

    // -- Timestamp in header defaults to http-date --

    static TS_HEADER_MEMBER: Schema = Schema::new_member(
        crate::shape_id!("test", "S"),
        ShapeType::Timestamp,
        "ifModified",
        0,
    )
    .with_http_header("If-Modified-Since");

    static TS_HEADER_SCHEMA: Schema = Schema::new_struct(
        crate::shape_id!("test", "S"),
        ShapeType::Structure,
        &[&TS_HEADER_MEMBER],
    );

    struct TimestampHeaderStruct;
    impl SerializableStruct for TimestampHeaderStruct {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_timestamp(&TS_HEADER_MEMBER, &aws_smithy_types::DateTime::from_secs(0))
        }
    }

    #[test]
    fn timestamp_header_uses_http_date() {
        let request = make_protocol()
            .serialize_request(
                &TimestampHeaderStruct,
                &TS_HEADER_SCHEMA,
                "https://example.com",
                &ConfigBag::base(),
            )
            .unwrap();
        let value = request.headers().get("If-Modified-Since").unwrap();
        // http-date format: "Thu, 01 Jan 1970 00:00:00 GMT"
        assert!(value.contains("1970"), "expected http-date, got: {value}");
    }

    // -- Timestamp in query defaults to date-time --

    static TS_QUERY_MEMBER: Schema = Schema::new_member(
        crate::shape_id!("test", "S"),
        ShapeType::Timestamp,
        "since",
        0,
    )
    .with_http_query("since");

    static TS_QUERY_SCHEMA: Schema = Schema::new_struct(
        crate::shape_id!("test", "S"),
        ShapeType::Structure,
        &[&TS_QUERY_MEMBER],
    );

    struct TimestampQueryStruct;
    impl SerializableStruct for TimestampQueryStruct {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_timestamp(&TS_QUERY_MEMBER, &aws_smithy_types::DateTime::from_secs(0))
        }
    }

    #[test]
    fn timestamp_query_uses_date_time() {
        let request = make_protocol()
            .serialize_request(
                &TimestampQueryStruct,
                &TS_QUERY_SCHEMA,
                "https://example.com",
                &ConfigBag::base(),
            )
            .unwrap();
        assert_eq!(
            request.uri(),
            "https://example.com?since=1970-01-01T00%3A00%3A00Z"
        );
    }

    // -- Unbound members go to body, bound members do not --

    static BOUND_MEMBER: Schema = Schema::new_member(
        crate::shape_id!("test", "S"),
        ShapeType::String,
        "headerVal",
        0,
    )
    .with_http_header("X-Val");
    static UNBOUND_MEMBER: Schema = Schema::new_member(
        crate::shape_id!("test", "S"),
        ShapeType::String,
        "bodyVal",
        1,
    );
    static MIXED_SCHEMA: Schema = Schema::new_struct(
        crate::shape_id!("test", "S"),
        ShapeType::Structure,
        &[&BOUND_MEMBER, &UNBOUND_MEMBER],
    );

    struct MixedStruct;
    impl SerializableStruct for MixedStruct {
        fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            s.write_string(&BOUND_MEMBER, "in-header")?;
            s.write_string(&UNBOUND_MEMBER, "in-body")
        }
    }

    #[test]
    fn bound_members_not_in_body() {
        let request = make_protocol()
            .serialize_request(
                &MixedStruct,
                &MIXED_SCHEMA,
                "https://example.com",
                &ConfigBag::base(),
            )
            .unwrap();
        let body = std::str::from_utf8(request.body().bytes().unwrap()).unwrap();
        assert!(
            body.contains("in-body"),
            "body should contain unbound member"
        );
        assert!(
            !body.contains("in-header"),
            "body should NOT contain header-bound member"
        );
        assert_eq!(request.headers().get("X-Val").unwrap(), "in-header");
    }
}
