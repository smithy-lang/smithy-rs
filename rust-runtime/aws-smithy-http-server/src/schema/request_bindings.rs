/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Request-binding interpretation for the REST protocols (2g, Design B).
//!
//! [`RestRequestDeserializer`] is the runtime composite [`ShapeDeserializer`]
//! presented to the generated input walker. Per member schema it routes:
//!
//! - `@httpLabel` — from the request path matched against the schema's
//!   `@http` URI template (a re-match; the router's match result carries no
//!   captures).
//! - `@httpQuery` / `@httpQueryParams` — from the parsed query string.
//! - `@httpHeader` / `@httpPrefixHeaders` — from the request headers, with
//!   the legacy `aws_smithy_http::header` comma/quote-aware parsing.
//! - `@httpPayload` — blob/string members read the raw body; structure,
//!   union, and document members read the body through the codec.
//! - everything else — delegated to the codec body deserializer.
//!
//! The generated walker is transport-blind: it drives `read_struct` into the
//! internal builder exactly as any nested structure's walker would.

use std::borrow::Cow;

use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
use aws_smithy_schema::{Schema, ShapeType};
use aws_smithy_types::date_time::Format;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};

// ============================================================================
// Percent-decoding and query parsing
// ============================================================================

/// Percent-decodes `input`, mirroring `percent_encoding::percent_decode_str`
/// as used by the legacy generated deserializers: malformed escape sequences
/// pass through unchanged; invalid UTF-8 after decoding is an error. `+` is
/// NOT treated as a space.
pub(crate) fn percent_decode(input: &str) -> Result<String, SerdeError> {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3);
            let decoded = hex.and_then(|h| {
                let hi = (h[0] as char).to_digit(16)?;
                let lo = (h[1] as char).to_digit(16)?;
                Some((hi * 16 + lo) as u8)
            });
            match decoded {
                Some(byte) => {
                    out.push(byte);
                    i += 3;
                    continue;
                }
                None => {
                    out.push(b'%');
                    i += 1;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out)
        .map_err(|_| SerdeError::invalid_input("request URI cannot be percent decoded into valid UTF-8"))
}

/// Parses a raw query string into decoded `(key, value)` pairs, preserving
/// order of appearance. A key without `=` gets an empty value.
pub(crate) fn parse_query_pairs(query: Option<&str>) -> Result<Vec<(String, String)>, SerdeError> {
    let mut pairs = Vec::new();
    let Some(query) = query else {
        return Ok(pairs);
    };
    for part in query.split('&') {
        if part.is_empty() {
            continue;
        }
        let (key, value) = match part.split_once('=') {
            Some((k, v)) => (k, v),
            None => (part, ""),
        };
        pairs.push((percent_decode(key)?, percent_decode(value)?));
    }
    Ok(pairs)
}

// ============================================================================
// URI label extraction
// ============================================================================

/// Extracts `@httpLabel` values from `path` by matching it against the
/// `@http` URI `template` (path portion only — any query-literal portion of
/// the template is ignored). Values are percent-decoded.
///
/// This is a re-match: the router has already accepted the request, so a
/// mismatch here indicates a schema/routing inconsistency and is an error.
pub(crate) fn extract_labels<'t>(template: &'t str, path: &str) -> Result<Vec<(&'t str, String)>, SerdeError> {
    enum Seg<'t> {
        Literal(&'t str),
        Label(&'t str),
        Greedy(&'t str),
    }

    let template_path = template.split('?').next().unwrap_or(template);
    let template_segs: Vec<Seg<'t>> = template_path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            if let Some(inner) = s.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                match inner.strip_suffix('+') {
                    Some(name) => Seg::Greedy(name),
                    None => Seg::Label(inner),
                }
            } else {
                Seg::Literal(s)
            }
        })
        .collect();
    let path_segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    let mut labels = Vec::new();
    let mismatch = || SerdeError::invalid_input("request URI does not match `@http` URI pattern");

    let greedy_pos = template_segs.iter().position(|s| matches!(s, Seg::Greedy(_)));
    match greedy_pos {
        None => {
            if path_segs.len() != template_segs.len() {
                return Err(mismatch());
            }
            for (seg, value) in template_segs.iter().zip(&path_segs) {
                match seg {
                    Seg::Literal(lit) => {
                        if lit != value {
                            return Err(mismatch());
                        }
                    }
                    Seg::Label(name) => labels.push((*name, percent_decode(value)?)),
                    Seg::Greedy(_) => unreachable!(),
                }
            }
        }
        Some(pos) => {
            let after = &template_segs[pos + 1..];
            // Segments before the greedy label match from the front; segments
            // after it match from the end; the middle (at least one segment)
            // is the greedy value.
            if path_segs.len() < pos + 1 + after.len() {
                return Err(mismatch());
            }
            for (seg, value) in template_segs[..pos].iter().zip(&path_segs) {
                match seg {
                    Seg::Literal(lit) => {
                        if lit != value {
                            return Err(mismatch());
                        }
                    }
                    Seg::Label(name) => labels.push((*name, percent_decode(value)?)),
                    Seg::Greedy(_) => unreachable!(),
                }
            }
            let tail_start = path_segs.len() - after.len();
            for (seg, value) in after.iter().zip(&path_segs[tail_start..]) {
                match seg {
                    Seg::Literal(lit) => {
                        if lit != value {
                            return Err(mismatch());
                        }
                    }
                    Seg::Label(name) => labels.push((*name, percent_decode(value)?)),
                    Seg::Greedy(_) => {
                        return Err(SerdeError::invalid_input(
                            "`@http` URI pattern cannot contain more than one greedy label",
                        ))
                    }
                }
            }
            let greedy_value = path_segs[pos..tail_start].join("/");
            if let Seg::Greedy(name) = template_segs[pos] {
                labels.push((name, percent_decode(&greedy_value)?));
            }
        }
    }
    Ok(labels)
}

// ============================================================================
// Timestamp format resolution
// ============================================================================

/// Where a bound value came from; determines the default timestamp format
/// (headers: `http-date`; query strings and labels: `date-time`).
#[derive(Copy, Clone, Debug)]
pub(crate) enum BindingLocation {
    Header,
    Query,
    Label,
}

fn resolve_timestamp_format(read_schema: &Schema<'_>, member: &Schema<'_>, location: BindingLocation) -> Format {
    use aws_smithy_schema::traits::TimestampFormat as SchemaFormat;
    let explicit = read_schema
        .timestamp_format()
        .or_else(|| member.timestamp_format())
        .map(|t| t.format());
    match explicit {
        Some(SchemaFormat::EpochSeconds) => Format::EpochSeconds,
        Some(SchemaFormat::HttpDate) => Format::HttpDate,
        Some(SchemaFormat::DateTime) => Format::DateTime,
        None => match location {
            BindingLocation::Header => Format::HttpDate,
            BindingLocation::Query | BindingLocation::Label => Format::DateTime,
        },
    }
}

fn parse_primitive<T: aws_smithy_types::primitive::Parse>(value: &str, what: &str) -> Result<T, SerdeError> {
    T::parse_smithy_primitive(value.trim()).map_err(|err| SerdeError::invalid_input(format!("invalid {what}: {err}")))
}

macro_rules! unsupported_reads {
    ($why:literal; $($method:ident -> $ret:ty),+ $(,)?) => {
        $(
            fn $method(&mut self, _schema: &Schema<'_>) -> Result<$ret, SerdeError> {
                Err(SerdeError::unsupported($why))
            }
        )+
    };
}

// ============================================================================
// Decoded string values (labels and query parameters)
// ============================================================================

/// Deserializer over pre-decoded string values for one `@httpQuery` or
/// `@httpLabel` member. Scalar reads take the LAST value (mirroring the
/// legacy last-occurrence-wins loop); list reads yield every value in order
/// of appearance.
pub(crate) struct DecodedValuesDeserializer<'a> {
    values: Vec<Cow<'a, str>>,
    member: &'a Schema<'a>,
    location: BindingLocation,
    /// `Some(idx)` while iterating a list; scalar reads otherwise.
    cursor: Option<usize>,
}

impl<'a> DecodedValuesDeserializer<'a> {
    pub(crate) fn new(values: Vec<Cow<'a, str>>, member: &'a Schema<'a>, location: BindingLocation) -> Self {
        debug_assert!(!values.is_empty());
        Self {
            values,
            member,
            location,
            cursor: None,
        }
    }

    fn current(&mut self) -> Result<&str, SerdeError> {
        match self.cursor {
            Some(idx) => {
                let value = self
                    .values
                    .get(idx)
                    .ok_or_else(|| SerdeError::invalid_input("list element read past the end"))?;
                self.cursor = Some(idx + 1);
                Ok(value)
            }
            // Scalar: last occurrence wins, matching the legacy generated
            // query-pair loop where each match overwrites the builder field.
            None => Ok(self.values.last().expect("constructed non-empty")),
        }
    }
}

impl ShapeDeserializer for DecodedValuesDeserializer<'_> {
    fn read_struct(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "structures cannot be bound to labels or query strings",
        ))
    }

    fn read_list(
        &mut self,
        _schema: &Schema<'_>,
        consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.cursor = Some(0);
        for _ in 0..self.values.len() {
            consumer(self)?;
        }
        self.cursor = None;
        Ok(())
    }

    fn read_map(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "maps cannot be bound to labels or query strings",
        ))
    }

    fn read_boolean(&mut self, _schema: &Schema<'_>) -> Result<bool, SerdeError> {
        let v = self.current()?;
        parse_primitive::<bool>(v, "boolean")
    }

    fn read_byte(&mut self, _schema: &Schema<'_>) -> Result<i8, SerdeError> {
        let v = self.current()?;
        parse_primitive::<i8>(v, "byte")
    }

    fn read_short(&mut self, _schema: &Schema<'_>) -> Result<i16, SerdeError> {
        let v = self.current()?;
        parse_primitive::<i16>(v, "short")
    }

    fn read_integer(&mut self, _schema: &Schema<'_>) -> Result<i32, SerdeError> {
        let v = self.current()?;
        parse_primitive::<i32>(v, "integer")
    }

    fn read_long(&mut self, _schema: &Schema<'_>) -> Result<i64, SerdeError> {
        let v = self.current()?;
        parse_primitive::<i64>(v, "long")
    }

    fn read_float(&mut self, _schema: &Schema<'_>) -> Result<f32, SerdeError> {
        let v = self.current()?;
        parse_primitive::<f32>(v, "float")
    }

    fn read_double(&mut self, _schema: &Schema<'_>) -> Result<f64, SerdeError> {
        let v = self.current()?;
        parse_primitive::<f64>(v, "double")
    }

    fn read_big_integer(&mut self, _schema: &Schema<'_>) -> Result<BigInteger, SerdeError> {
        use std::str::FromStr;
        let v = self.current()?;
        BigInteger::from_str(v.trim()).map_err(|_| SerdeError::invalid_input(format!("invalid big integer: {v}")))
    }

    fn read_big_decimal(&mut self, _schema: &Schema<'_>) -> Result<BigDecimal, SerdeError> {
        use std::str::FromStr;
        let v = self.current()?;
        BigDecimal::from_str(v.trim()).map_err(|_| SerdeError::invalid_input(format!("invalid big decimal: {v}")))
    }

    fn read_string(&mut self, _schema: &Schema<'_>) -> Result<String, SerdeError> {
        Ok(self.current()?.to_string())
    }

    fn read_blob(&mut self, _schema: &Schema<'_>) -> Result<Blob, SerdeError> {
        let v = self.current()?.to_string();
        let decoded = aws_smithy_types::base64::decode(&v)
            .map_err(|err| SerdeError::invalid_input(format!("invalid base64: {err}")))?;
        Ok(Blob::new(decoded))
    }

    fn read_timestamp(&mut self, schema: &Schema<'_>) -> Result<DateTime, SerdeError> {
        let format = resolve_timestamp_format(schema, self.member, self.location);
        let v = self.current()?.to_string();
        DateTime::from_str(&v, format).map_err(|err| SerdeError::invalid_input(format!("invalid timestamp: {err}")))
    }

    fn read_document(&mut self, _schema: &Schema<'_>) -> Result<Document, SerdeError> {
        Err(SerdeError::unsupported(
            "documents cannot be bound to labels or query strings",
        ))
    }

    fn is_null(&self) -> bool {
        false
    }

    fn container_size(&self) -> Option<usize> {
        Some(self.values.len())
    }
}

// ============================================================================
// Header values
// ============================================================================

/// Deserializer over the raw values of one `@httpHeader`-bound member,
/// applying the legacy `aws_smithy_http::header` parsing semantics
/// (comma/quote-aware list splitting, `many_dates` for timestamp lists,
/// single-instance rule for scalar strings).
pub(crate) struct HeaderValuesDeserializer<'a> {
    /// One entry per header instance (a repeated header name yields several).
    values: Vec<&'a str>,
    member: &'a Schema<'a>,
    /// Tokenized list elements, populated by `read_list`.
    tokens: Vec<HeaderToken>,
    cursor: Option<usize>,
}

enum HeaderToken {
    Text(String),
    Date(DateTime),
}

impl<'a> HeaderValuesDeserializer<'a> {
    pub(crate) fn new(values: Vec<&'a str>, member: &'a Schema<'a>) -> Self {
        debug_assert!(!values.is_empty());
        Self {
            values,
            member,
            tokens: Vec::new(),
            cursor: None,
        }
    }

    fn next_token(&mut self) -> Result<&HeaderToken, SerdeError> {
        let idx = self
            .cursor
            .ok_or_else(|| SerdeError::invalid_input("header list element read outside a list"))?;
        self.cursor = Some(idx + 1);
        self.tokens
            .get(idx)
            .ok_or_else(|| SerdeError::invalid_input("header list element read past the end"))
    }

    fn next_text(&mut self) -> Result<String, SerdeError> {
        match self.next_token()? {
            HeaderToken::Text(s) => Ok(s.clone()),
            HeaderToken::Date(_) => Err(SerdeError::invalid_input(
                "expected a string header element, found a timestamp",
            )),
        }
    }

    /// The single raw value for a scalar read. Mirrors
    /// `aws_smithy_http::header::one_or_none`: more than one header instance
    /// is an error.
    fn single_value(&self) -> Result<&'a str, SerdeError> {
        if self.values.len() > 1 {
            return Err(SerdeError::invalid_input(
                "expected a single header value but found multiple",
            ));
        }
        Ok(self.values[0])
    }
}

impl ShapeDeserializer for HeaderValuesDeserializer<'_> {
    fn read_struct(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported("structures cannot be bound to headers"))
    }

    fn read_list(
        &mut self,
        schema: &Schema<'_>,
        consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        // Element schema (when resolvable) decides the tokenization:
        // timestamps use `many_dates` (comma-aware `http-date` parsing);
        // everything else uses the RFC-7230 quote-aware splitter.
        let element = schema.member();
        let element_is_timestamp = element.map(|e| e.shape_type() == ShapeType::Timestamp).unwrap_or(false);
        self.tokens = if element_is_timestamp {
            let format =
                resolve_timestamp_format(element.expect("checked above"), self.member, BindingLocation::Header);
            aws_smithy_http::header::many_dates(self.values.iter().copied(), format)
                .map_err(|err| SerdeError::invalid_input(format!("{err}")))?
                .into_iter()
                .map(HeaderToken::Date)
                .collect()
        } else {
            aws_smithy_http::header::read_many_from_str::<String>(self.values.iter().copied())
                .map_err(|err| SerdeError::invalid_input(format!("{err}")))?
                .into_iter()
                .map(HeaderToken::Text)
                .collect()
        };
        self.cursor = Some(0);
        for _ in 0..self.tokens.len() {
            consumer(self)?;
        }
        self.cursor = None;
        Ok(())
    }

    fn read_map(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "maps cannot be bound to a single header (`@httpPrefixHeaders` is a map binding)",
        ))
    }

    fn read_boolean(&mut self, _schema: &Schema<'_>) -> Result<bool, SerdeError> {
        match self.cursor {
            Some(_) => parse_primitive::<bool>(&self.next_text()?, "boolean"),
            None => parse_primitive::<bool>(self.single_value()?, "boolean"),
        }
    }

    fn read_byte(&mut self, _schema: &Schema<'_>) -> Result<i8, SerdeError> {
        match self.cursor {
            Some(_) => parse_primitive::<i8>(&self.next_text()?, "byte"),
            None => parse_primitive::<i8>(self.single_value()?, "byte"),
        }
    }

    fn read_short(&mut self, _schema: &Schema<'_>) -> Result<i16, SerdeError> {
        match self.cursor {
            Some(_) => parse_primitive::<i16>(&self.next_text()?, "short"),
            None => parse_primitive::<i16>(self.single_value()?, "short"),
        }
    }

    fn read_integer(&mut self, _schema: &Schema<'_>) -> Result<i32, SerdeError> {
        match self.cursor {
            Some(_) => parse_primitive::<i32>(&self.next_text()?, "integer"),
            None => parse_primitive::<i32>(self.single_value()?, "integer"),
        }
    }

    fn read_long(&mut self, _schema: &Schema<'_>) -> Result<i64, SerdeError> {
        match self.cursor {
            Some(_) => parse_primitive::<i64>(&self.next_text()?, "long"),
            None => parse_primitive::<i64>(self.single_value()?, "long"),
        }
    }

    fn read_float(&mut self, _schema: &Schema<'_>) -> Result<f32, SerdeError> {
        match self.cursor {
            Some(_) => parse_primitive::<f32>(&self.next_text()?, "float"),
            None => parse_primitive::<f32>(self.single_value()?, "float"),
        }
    }

    fn read_double(&mut self, _schema: &Schema<'_>) -> Result<f64, SerdeError> {
        match self.cursor {
            Some(_) => parse_primitive::<f64>(&self.next_text()?, "double"),
            None => parse_primitive::<f64>(self.single_value()?, "double"),
        }
    }

    fn read_big_integer(&mut self, _schema: &Schema<'_>) -> Result<BigInteger, SerdeError> {
        use std::str::FromStr;
        let v = match self.cursor {
            Some(_) => self.next_text()?,
            None => self.single_value()?.trim().to_string(),
        };
        BigInteger::from_str(&v).map_err(|_| SerdeError::invalid_input(format!("invalid big integer: {v}")))
    }

    fn read_big_decimal(&mut self, _schema: &Schema<'_>) -> Result<BigDecimal, SerdeError> {
        use std::str::FromStr;
        let v = match self.cursor {
            Some(_) => self.next_text()?,
            None => self.single_value()?.trim().to_string(),
        };
        BigDecimal::from_str(&v).map_err(|_| SerdeError::invalid_input(format!("invalid big decimal: {v}")))
    }

    fn read_string(&mut self, schema: &Schema<'_>) -> Result<String, SerdeError> {
        let raw = match self.cursor {
            Some(_) => self.next_text()?,
            // Scalar strings use the full single value (no comma splitting),
            // trimmed — matching `one_or_none::<String>`.
            None => self.single_value()?.trim().to_string(),
        };
        // `@mediaType` on a header-bound string travels base64-encoded.
        let media_typed = schema.media_type().is_some() || self.member.media_type().is_some();
        if media_typed {
            let decoded = aws_smithy_types::base64::decode(&raw)
                .map_err(|err| SerdeError::invalid_input(format!("invalid base64: {err}")))?;
            String::from_utf8(decoded)
                .map_err(|_| SerdeError::invalid_input("base64-decoded header was not valid UTF-8"))
        } else {
            Ok(raw)
        }
    }

    fn read_blob(&mut self, _schema: &Schema<'_>) -> Result<Blob, SerdeError> {
        let v = match self.cursor {
            Some(_) => self.next_text()?,
            None => self.single_value()?.trim().to_string(),
        };
        let decoded = aws_smithy_types::base64::decode(&v)
            .map_err(|err| SerdeError::invalid_input(format!("invalid base64: {err}")))?;
        Ok(Blob::new(decoded))
    }

    fn read_timestamp(&mut self, schema: &Schema<'_>) -> Result<DateTime, SerdeError> {
        match self.cursor {
            Some(_) => match self.next_token()? {
                HeaderToken::Date(dt) => Ok(*dt),
                HeaderToken::Text(s) => {
                    let s = s.clone();
                    let format = resolve_timestamp_format(schema, self.member, BindingLocation::Header);
                    DateTime::from_str(&s, format)
                        .map_err(|err| SerdeError::invalid_input(format!("invalid timestamp: {err}")))
                }
            },
            None => {
                let format = resolve_timestamp_format(schema, self.member, BindingLocation::Header);
                let dates = aws_smithy_http::header::many_dates(self.values.iter().copied(), format)
                    .map_err(|err| SerdeError::invalid_input(format!("{err}")))?;
                match dates.len() {
                    1 => Ok(dates[0]),
                    0 => Err(SerdeError::invalid_input("expected a timestamp header value")),
                    _ => Err(SerdeError::invalid_input(
                        "expected a single timestamp header value but found multiple",
                    )),
                }
            }
        }
    }

    fn read_document(&mut self, _schema: &Schema<'_>) -> Result<Document, SerdeError> {
        Err(SerdeError::unsupported("documents cannot be bound to headers"))
    }

    fn is_null(&self) -> bool {
        false
    }

    fn container_size(&self) -> Option<usize> {
        self.cursor.map(|_| self.tokens.len())
    }
}

// ============================================================================
// Prefix headers and query-params maps
// ============================================================================

/// Deserializer for one `@httpPrefixHeaders`-bound map member: each request
/// header starting with the prefix becomes a `suffix → value` entry.
pub(crate) struct StringMapDeserializer {
    entries: Vec<(String, Vec<String>)>,
    cursor: usize,
    element_cursor: Option<usize>,
}

impl StringMapDeserializer {
    pub(crate) fn new(entries: Vec<(String, Vec<String>)>) -> Self {
        Self {
            entries,
            cursor: 0,
            element_cursor: None,
        }
    }

    fn current_values(&self) -> Result<&Vec<String>, SerdeError> {
        self.entries
            .get(self.cursor)
            .map(|(_, v)| v)
            .ok_or_else(|| SerdeError::invalid_input("map value read without a current entry"))
    }
}

impl ShapeDeserializer for StringMapDeserializer {
    fn read_struct(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "structures cannot appear in header/query-bound maps",
        ))
    }

    fn read_list(
        &mut self,
        _schema: &Schema<'_>,
        consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        // Map<String, List<String>> for `@httpQueryParams`: every value for
        // the current key, in order of appearance.
        let count = self.current_values()?.len();
        self.element_cursor = Some(0);
        for _ in 0..count {
            consumer(self)?;
        }
        self.element_cursor = None;
        Ok(())
    }

    fn read_map(
        &mut self,
        _schema: &Schema<'_>,
        consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        for idx in 0..self.entries.len() {
            self.cursor = idx;
            let key = self.entries[idx].0.clone();
            consumer(key, self)?;
        }
        Ok(())
    }

    fn read_string(&mut self, _schema: &Schema<'_>) -> Result<String, SerdeError> {
        match self.element_cursor {
            Some(idx) => {
                let value = self
                    .current_values()?
                    .get(idx)
                    .cloned()
                    .ok_or_else(|| SerdeError::invalid_input("list element read past the end"))?;
                self.element_cursor = Some(idx + 1);
                Ok(value)
            }
            // Scalar map value: last occurrence wins (legacy insert loop).
            None => self
                .current_values()?
                .last()
                .cloned()
                .ok_or_else(|| SerdeError::invalid_input("map value read without a value")),
        }
    }

    unsupported_reads! {
        "header/query-bound map values are strings";
        read_boolean -> bool,
        read_byte -> i8,
        read_short -> i16,
        read_integer -> i32,
        read_long -> i64,
        read_float -> f32,
        read_double -> f64,
        read_big_integer -> BigInteger,
        read_big_decimal -> BigDecimal,
        read_blob -> Blob,
        read_timestamp -> DateTime,
        read_document -> Document,
    }

    fn is_null(&self) -> bool {
        false
    }

    fn container_size(&self) -> Option<usize> {
        match self.element_cursor {
            Some(_) => self.current_values().ok().map(|v| v.len()),
            None => Some(self.entries.len()),
        }
    }
}

// ============================================================================
// Raw payload
// ============================================================================

/// Deserializer for a blob/string `@httpPayload` member: the body bytes ARE
/// the value.
pub(crate) struct PayloadBytesDeserializer<'a> {
    body: &'a [u8],
}

impl<'a> PayloadBytesDeserializer<'a> {
    pub(crate) fn new(body: &'a [u8]) -> Self {
        Self { body }
    }
}

impl ShapeDeserializer for PayloadBytesDeserializer<'_> {
    fn read_struct(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "structure payloads read through the protocol codec, not raw bytes",
        ))
    }

    fn read_list(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported("lists cannot be a raw payload"))
    }

    fn read_map(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported("maps cannot be a raw payload"))
    }

    fn read_string(&mut self, _schema: &Schema<'_>) -> Result<String, SerdeError> {
        std::str::from_utf8(self.body)
            .map(|s| s.to_string())
            .map_err(|_| SerdeError::invalid_input("string payload was not valid UTF-8"))
    }

    fn read_blob(&mut self, _schema: &Schema<'_>) -> Result<Blob, SerdeError> {
        Ok(Blob::new(self.body.to_vec()))
    }

    unsupported_reads! {
        "raw payloads are blobs or strings";
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

// ============================================================================
// Empty struct (empty request bodies on the RPC protocols)
// ============================================================================

/// A deserializer for an absent request body: `read_struct` invokes the
/// consumer for no members, leaving every builder field unset (`@required`
/// enforcement happens in `build()`).
pub(crate) struct EmptyStructDeserializer;

impl ShapeDeserializer for EmptyStructDeserializer {
    fn read_struct(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Ok(())
    }

    fn read_list(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::invalid_input("expected a structure"))
    }

    fn read_map(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::invalid_input("expected a structure"))
    }

    unsupported_reads! {
        "empty request body";
        read_boolean -> bool,
        read_byte -> i8,
        read_short -> i16,
        read_integer -> i32,
        read_long -> i64,
        read_float -> f32,
        read_double -> f64,
        read_big_integer -> BigInteger,
        read_big_decimal -> BigDecimal,
        read_blob -> Blob,
        read_timestamp -> DateTime,
        read_document -> Document,
    }

    fn read_string(&mut self, _schema: &Schema<'_>) -> Result<String, SerdeError> {
        Err(SerdeError::unsupported("empty request body"))
    }

    fn is_null(&self) -> bool {
        true
    }

    fn container_size(&self) -> Option<usize> {
        Some(0)
    }
}

// ============================================================================
// The composite
// ============================================================================

/// The composite request deserializer for REST protocols: routes each member
/// of the operation input schema to its transport location, delegating
/// unbound members to the codec body deserializer.
pub(crate) struct RestRequestDeserializer<'a, C> {
    codec: &'a C,
    headers: &'a http::HeaderMap,
    uri: &'a http::Uri,
    body: &'a [u8],
}

impl<'a, C: Codec> RestRequestDeserializer<'a, C> {
    pub(crate) fn new(codec: &'a C, parts: &'a http::request::Parts, body: &'a [u8]) -> Self {
        Self {
            codec,
            headers: &parts.headers,
            uri: &parts.uri,
            body,
        }
    }

    pub(crate) fn from_request(codec: &'a C, request: &'a http::Request<bytes::Bytes>) -> Self {
        Self {
            codec,
            headers: request.headers(),
            uri: request.uri(),
            body: request.body().as_ref(),
        }
    }

    fn header_values(&self, name: &str) -> Result<Vec<&'a str>, SerdeError> {
        let mut values = Vec::new();
        for value in self.headers.get_all(name) {
            values.push(
                value
                    .to_str()
                    .map_err(|_| SerdeError::invalid_input(format!("header `{name}` was not valid UTF-8")))?,
            );
        }
        Ok(values)
    }
}

impl<C: Codec> ShapeDeserializer for RestRequestDeserializer<'_, C> {
    fn read_struct(
        &mut self,
        schema: &Schema<'_>,
        consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        // Labels and query pairs are parsed once, up front.
        let has_labels = schema.members().iter().any(|m| m.http_label().is_some());
        let labels: Vec<(&str, String)> = if has_labels {
            let template = schema
                .http()
                .map(|h| h.uri())
                .ok_or_else(|| SerdeError::invalid_input("input schema has @httpLabel members but no @http trait"))?;
            extract_labels(template, self.uri.path())?
        } else {
            Vec::new()
        };
        let needs_query = schema
            .members()
            .iter()
            .any(|m| m.http_query().is_some() || m.http_query_params().is_some());
        let query_pairs: Vec<(String, String)> = if needs_query {
            parse_query_pairs(self.uri.query())?
        } else {
            Vec::new()
        };

        let mut has_unbound_members = false;
        for member in schema.members() {
            let member_name = member.member_name().unwrap_or_default();
            if member.http_label().is_some() {
                let value = labels
                    .iter()
                    .find(|(name, _)| *name == member_name)
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| {
                        SerdeError::invalid_input(format!("no `{{{member_name}}}` label in the `@http` URI pattern"))
                    })?;
                let mut deser = DecodedValuesDeserializer::new(vec![Cow::Owned(value)], member, BindingLocation::Label);
                consumer(member, &mut deser)?;
            } else if let Some(query) = member.http_query() {
                let values: Vec<Cow<'_, str>> = query_pairs
                    .iter()
                    .filter(|(k, _)| k == query.value())
                    .map(|(_, v)| Cow::Borrowed(v.as_str()))
                    .collect();
                if !values.is_empty() {
                    let mut deser = DecodedValuesDeserializer::new(values, member, BindingLocation::Query);
                    consumer(member, &mut deser)?;
                }
            } else if member.http_query_params().is_some() {
                // Servers put ALL query parameters in the map, including ones
                // also bound to explicit `@httpQuery` members.
                let mut entries: Vec<(String, Vec<String>)> = Vec::new();
                for (k, v) in &query_pairs {
                    match entries.iter_mut().find(|(key, _)| key == k) {
                        Some((_, values)) => values.push(v.clone()),
                        None => entries.push((k.clone(), vec![v.clone()])),
                    }
                }
                let mut deser = StringMapDeserializer::new(entries);
                consumer(member, &mut deser)?;
            } else if let Some(header) = member.http_header() {
                let values = self.header_values(header.value())?;
                if !values.is_empty() {
                    let mut deser = HeaderValuesDeserializer::new(values, member);
                    consumer(member, &mut deser)?;
                }
            } else if let Some(prefix) = member.http_prefix_headers() {
                let prefix = prefix.value();
                let names: Vec<&str> = self.headers.keys().map(|n| n.as_str()).collect();
                let mut entries: Vec<(String, Vec<String>)> = Vec::new();
                for (suffix, full_name) in aws_smithy_http::header::headers_for_prefix(names.into_iter(), prefix) {
                    let values: Vec<String> = self
                        .header_values(full_name)?
                        .into_iter()
                        .map(|v| v.to_string())
                        .collect();
                    if !values.is_empty() {
                        entries.push((suffix.to_string(), values));
                    }
                }
                if !entries.is_empty() {
                    let mut deser = StringMapDeserializer::new(entries);
                    consumer(member, &mut deser)?;
                }
            } else if member.http_payload().is_some() {
                match member.shape_type() {
                    ShapeType::Blob | ShapeType::String => {
                        // Legacy parity (`HttpBindingGenerator.kt:342`): an empty
                        // body leaves a raw blob/string payload member UNSET.
                        if !self.body.is_empty() {
                            let mut deser = PayloadBytesDeserializer::new(self.body);
                            consumer(member, &mut deser)?;
                        }
                    }
                    _ => {
                        // Structure / union / document payload: the body IS
                        // that member's codec document. An empty body leaves
                        // the member unset.
                        if !self.body.is_empty() {
                            let mut deser = self.codec.create_deserializer(self.body);
                            consumer(member, &mut deser)?;
                        }
                    }
                }
            } else {
                has_unbound_members = true;
            }
        }

        // Unbound members come from the codec body document, in wire order.
        if has_unbound_members && !self.body.is_empty() {
            let mut body_deser = self.codec.create_deserializer(self.body);
            body_deser.read_struct(schema, consumer)?;
        }
        Ok(())
    }

    fn read_list(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported("operation input must be a structure"))
    }

    fn read_map(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported("operation input must be a structure"))
    }

    unsupported_reads! {
        "operation input must be a structure";
        read_boolean -> bool,
        read_byte -> i8,
        read_short -> i16,
        read_integer -> i32,
        read_long -> i64,
        read_float -> f32,
        read_double -> f64,
        read_big_integer -> BigInteger,
        read_big_decimal -> BigDecimal,
        read_blob -> Blob,
        read_timestamp -> DateTime,
        read_document -> Document,
    }

    fn read_string(&mut self, _schema: &Schema<'_>) -> Result<String, SerdeError> {
        Err(SerdeError::unsupported("operation input must be a structure"))
    }

    fn is_null(&self) -> bool {
        false
    }

    fn container_size(&self) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
    use aws_smithy_schema::traits::HttpTrait;
    use aws_smithy_schema::ShapeId;

    #[test]
    fn uri_parsing() {
        // Percent-decoding: `+` is not a space, malformed escapes pass
        // through, invalid UTF-8 errors.
        assert_eq!(percent_decode("a%20b%2Fc+d").unwrap(), "a b/c+d");
        assert_eq!(percent_decode("100%zz").unwrap(), "100%zz");
        assert!(percent_decode("%FF").is_err());

        // Query pairs decode in order; a key without `=` gets "".
        assert_eq!(
            parse_query_pairs(Some("a=1&b=x%20y&flag&c=")).unwrap(),
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "x y".to_string()),
                ("flag".to_string(), "".to_string()),
                ("c".to_string(), "".to_string()),
            ]
        );
        assert!(parse_query_pairs(None).unwrap().is_empty());

        // Labels: plain (decoded), greedy (keeps slashes), greedy with a
        // literal suffix, query-literal templates ignored, mismatches error.
        assert_eq!(
            extract_labels("/pets/{name}/{age}", "/pets/rex%20jr/7").unwrap(),
            vec![("name", "rex jr".to_string()), ("age", "7".to_string())]
        );
        assert_eq!(
            extract_labels("/data/{key+}/meta", "/data/a/b/meta").unwrap(),
            vec![("key", "a/b".to_string())]
        );
        assert_eq!(
            extract_labels("/op/{id}?enabled", "/op/5").unwrap(),
            vec![("id", "5".to_string())]
        );
        assert!(extract_labels("/pets/{name}", "/people/rex").is_err());
        assert!(extract_labels("/pets/{name}", "/pets/rex/extra").is_err());
        assert!(extract_labels("/data/{key+}/meta", "/data/a/b/other").is_err());
    }

    // ------------------------------------------------------------------
    // Composite deserializer
    // ------------------------------------------------------------------

    static NAME_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#Input$name", "test", "Input"),
        ShapeType::String,
        "name",
        0,
    )
    .with_http_label();
    static AGE_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#Input$age", "test", "Input"),
        ShapeType::Integer,
        "age",
        1,
    )
    .with_http_query("age");
    static TAGS_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#Input$tags", "test", "Input"),
        ShapeType::List,
        "tags",
        2,
    )
    .with_http_query("tag");
    static TOKEN_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#Input$token", "test", "Input"),
        ShapeType::String,
        "token",
        3,
    )
    .with_http_header("x-token");
    static META_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#Input$meta", "test", "Input"),
        ShapeType::Map,
        "meta",
        4,
    )
    .with_http_prefix_headers("x-meta-");
    static NOTE_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#Input$note", "test", "Input"),
        ShapeType::String,
        "note",
        5,
    );
    static INPUT_MEMBERS: [&Schema<'static>; 6] = [
        &NAME_MEMBER,
        &AGE_MEMBER,
        &TAGS_MEMBER,
        &TOKEN_MEMBER,
        &META_MEMBER,
        &NOTE_MEMBER,
    ];
    static INPUT_SCHEMA: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#Input", "test", "Input"),
        ShapeType::Structure,
        &INPUT_MEMBERS,
    )
    .with_http(HttpTrait::new("POST", "/pets/{name}", Some(200)));

    fn json_codec() -> JsonCodec {
        JsonCodec::new(
            JsonCodecSettings::builder()
                .use_json_name(true)
                .default_timestamp_format(aws_smithy_types::date_time::Format::EpochSeconds)
                .build(),
        )
    }

    #[derive(Debug, Default, PartialEq)]
    struct Collected {
        name: Option<String>,
        age: Option<i32>,
        tags: Vec<String>,
        token: Option<String>,
        meta: Vec<(String, String)>,
        note: Option<String>,
    }

    fn collect(parts: &http::request::Parts, body: &[u8]) -> Result<Collected, SerdeError> {
        let codec = json_codec();
        let mut deser = RestRequestDeserializer::new(&codec, parts, body);
        let mut out = Collected::default();
        deser.read_struct(&INPUT_SCHEMA, &mut |member, d| {
            match member.member_index() {
                Some(0) => out.name = Some(d.read_string(member)?),
                Some(1) => out.age = Some(d.read_integer(member)?),
                Some(2) => {
                    let mut tags = Vec::new();
                    d.read_list(member, &mut |element| {
                        tags.push(element.read_string(member)?);
                        Ok(())
                    })?;
                    out.tags = tags;
                }
                Some(3) => out.token = Some(d.read_string(member)?),
                Some(4) => {
                    let mut meta = Vec::new();
                    d.read_map(member, &mut |key, value| {
                        meta.push((key, value.read_string(member)?));
                        Ok(())
                    })?;
                    out.meta = meta;
                }
                Some(5) => out.note = Some(d.read_string(member)?),
                _ => {}
            }
            Ok(())
        })?;
        Ok(out)
    }

    fn request_parts(uri: &str, headers: &[(&'static str, &str)]) -> http::request::Parts {
        let mut builder = http::Request::builder().method("POST").uri(uri);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(()).unwrap().into_parts().0
    }

    #[test]
    fn composite_deserializer() {
        // Every binding location routes: label, query scalar + list, header,
        // prefix headers, body member via the codec.
        let parts = request_parts(
            "/pets/rex?age=7&tag=a&tag=b",
            &[("x-token", "secret"), ("x-meta-color", "red"), ("x-meta-size", "xl")],
        );
        let out = collect(&parts, br#"{"note":"hello"}"#).unwrap();
        assert_eq!(out.name.as_deref(), Some("rex"));
        assert_eq!(out.age, Some(7));
        assert_eq!(out.tags, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(out.token.as_deref(), Some("secret"));
        let mut meta = out.meta.clone();
        meta.sort();
        assert_eq!(
            meta,
            vec![
                ("color".to_string(), "red".to_string()),
                ("size".to_string(), "xl".to_string())
            ]
        );
        assert_eq!(out.note.as_deref(), Some("hello"));

        // Absent bindings leave members unset (`@required` enforcement stays
        // in `build()`).
        let parts = request_parts("/pets/rex", &[]);
        let out = collect(&parts, b"").unwrap();
        assert_eq!(out.name.as_deref(), Some("rex"));
        assert_eq!((out.age, out.token, out.note), (None, None, None));
        assert!(out.tags.is_empty());

        // Scalar query members: last occurrence wins (legacy loop semantics).
        let parts = request_parts("/pets/rex?age=1&age=2", &[]);
        assert_eq!(collect(&parts, b"").unwrap().age, Some(2));

        // Unparseable bound values are wire-level errors.
        let parts = request_parts("/pets/rex?age=notanumber", &[]);
        assert!(collect(&parts, b"").is_err());
    }
}
