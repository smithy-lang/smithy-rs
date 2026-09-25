/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Request-binding interpretation for the REST protocols.
//!
//! [`RestRequestDeserializer`] is the runtime composite [`ShapeDeserializer`]
//! presented to the generated input walker. Per member schema it routes:
//!
//! - `@httpLabel` — from the request path matched against the operation's
//!   `@http` URI template (a re-match; the router's match result carries no
//!   captures).
//! - `@httpQuery` / `@httpQueryParams` — from the parsed query string.
//! - `@httpHeader` / `@httpPrefixHeaders` — from the request headers, with
//!   the `aws_smithy_http::header` comma/quote-aware parsing.
//! - `@httpPayload` — blob/string members read the raw body; structure,
//!   union, and document members read the body through the codec.
//! - everything else — delegated to the codec body deserializer.
//!
//! The generated walker is transport-blind: it drives `read_struct` into the
//! internal builder exactly as any nested structure's walker would.

use std::borrow::Cow;
use std::collections::{hash_map::Entry, HashMap, HashSet};

use aws_smithy_runtime_api::http::{Headers, Uri};
use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
use aws_smithy_schema::{Schema, ShapeType};
use aws_smithy_types::date_time::Format;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};

/// `true` when `member` travels in the body rather than in the URI or headers. An `@httpPayload`
/// member counts: it *is* the body.
pub(crate) fn is_body_member(member: &Schema<'_>) -> bool {
    member.http_header().is_none()
        && member.http_query().is_none()
        && member.http_label().is_none()
        && member.http_prefix_headers().is_none()
        && member.http_query_params().is_none()
}

// ============================================================================
// Percent-decoding and query parsing
// ============================================================================

/// Percent-decodes a `@httpLabel` value: malformed escape sequences pass through unchanged, `+`
/// is NOT a space, and invalid UTF-8 after decoding rejects the request.
pub(crate) fn percent_decode(input: &str) -> Result<String, SerdeError> {
    percent_encoding::percent_decode_str(input)
        .decode_utf8()
        .map(Cow::into_owned)
        .map_err(|_| SerdeError::invalid_input("request URI cannot be percent decoded into valid UTF-8"))
}

/// Parses a raw query string into decoded `(key, value)` pairs with form-urlencoded semantics:
/// order of appearance is preserved, a key without `=` gets an empty value, `+` decodes to a
/// space, and invalid UTF-8 decodes lossily rather than failing.
pub(crate) fn parse_query_pairs(query: Option<&str>) -> Vec<(String, String)> {
    let Some(query) = query else {
        return Vec::new();
    };
    form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

// ============================================================================
// URI label extraction
// ============================================================================

/// The value a `@httpLabel` member reads from its raw path segment. Like legacy
/// (`ServerHttpBoundProtocolGenerator.generateParseStrFn`), string (and enum) and timestamp labels
/// are percent-decoded, while number and boolean labels are parsed from the raw segment, so
/// `%37` is not the integer 7.
pub(crate) fn label_value(member: &Schema<'_>, raw: &str) -> Result<String, SerdeError> {
    match member.shape_type() {
        ShapeType::String | ShapeType::Timestamp => percent_decode(raw),
        _ => Ok(raw.to_string()),
    }
}

/// Extracts `@httpLabel` values from `path` by matching it against the
/// `@http` URI `template` (path portion only — any query-literal portion of
/// the template is ignored). Values are returned raw, as they appear in the path: the reader
/// percent-decodes them only for members that legacy decodes (see [`label_value`]).
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
        .strip_prefix('/')
        .unwrap_or(template_path)
        .split('/')
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
    let path_segs: Vec<&str> = path.strip_prefix('/').unwrap_or(path).split('/').collect();

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
                    Seg::Label(name) => labels.push((*name, value.to_string())),
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
                    Seg::Label(name) => labels.push((*name, value.to_string())),
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
                    Seg::Label(name) => labels.push((*name, value.to_string())),
                    Seg::Greedy(_) => {
                        return Err(SerdeError::invalid_input(
                            "`@http` URI pattern cannot contain more than one greedy label",
                        ))
                    }
                }
            }
            let greedy_value = path_segs[pos..tail_start].join("/");
            if let Seg::Greedy(name) = template_segs[pos] {
                labels.push((name, greedy_value));
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

/// Parses a primitive from its wire text as-is. Header values arrive already
/// trimmed by the header tokenizer; label and query values are parsed untrimmed,
/// matching the legacy `parse_smithy_primitive(&value)` on the decoded segment.
fn parse_primitive<T: aws_smithy_types::primitive::Parse>(value: &str, what: &str) -> Result<T, SerdeError> {
    T::parse_smithy_primitive(value).map_err(|err| SerdeError::invalid_input(format!("invalid {what}: {err}")))
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
/// `@httpLabel` member. Scalar reads take the FIRST value (first occurrence
/// wins); list reads yield every value in order of appearance.
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
            // Scalar: first occurrence wins.
            None => Ok(self.values.first().expect("constructed non-empty")),
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
        BigInteger::from_str(v).map_err(|_| SerdeError::invalid_input(format!("invalid big integer: {v}")))
    }

    fn read_big_decimal(&mut self, _schema: &Schema<'_>) -> Result<BigDecimal, SerdeError> {
        use std::str::FromStr;
        let v = self.current()?;
        BigDecimal::from_str(v).map_err(|_| SerdeError::invalid_input(format!("invalid big decimal: {v}")))
    }

    fn read_string(&mut self, _schema: &Schema<'_>) -> Result<String, SerdeError> {
        Ok(self.current()?.to_string())
    }

    fn read_blob(&mut self, _schema: &Schema<'_>) -> Result<Blob, SerdeError> {
        Err(SerdeError::unsupported(
            "blobs cannot be bound to labels or query strings",
        ))
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
/// applying the `aws_smithy_http::header` parsing semantics
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

    /// Tokenizes scalar primitives just as legacy `read_many_primitive` does.
    fn primitive_value<T: aws_smithy_types::primitive::Parse>(&self) -> Result<T, SerdeError> {
        let mut values = aws_smithy_http::header::read_many_primitive::<T>(self.values.iter().copied())
            .map_err(|e| SerdeError::invalid_input(e.to_string()))?;
        if values.len() != 1 {
            return Err(SerdeError::invalid_input("expected one primitive header value"));
        }
        Ok(values.remove(0))
    }

    /// The single raw value for a scalar string, matching `one_or_none`.
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
            None => self.primitive_value::<bool>(),
        }
    }

    fn read_byte(&mut self, _schema: &Schema<'_>) -> Result<i8, SerdeError> {
        match self.cursor {
            Some(_) => parse_primitive::<i8>(&self.next_text()?, "byte"),
            None => self.primitive_value::<i8>(),
        }
    }

    fn read_short(&mut self, _schema: &Schema<'_>) -> Result<i16, SerdeError> {
        match self.cursor {
            Some(_) => parse_primitive::<i16>(&self.next_text()?, "short"),
            None => self.primitive_value::<i16>(),
        }
    }

    fn read_integer(&mut self, _schema: &Schema<'_>) -> Result<i32, SerdeError> {
        match self.cursor {
            Some(_) => parse_primitive::<i32>(&self.next_text()?, "integer"),
            None => self.primitive_value::<i32>(),
        }
    }

    fn read_long(&mut self, _schema: &Schema<'_>) -> Result<i64, SerdeError> {
        match self.cursor {
            Some(_) => parse_primitive::<i64>(&self.next_text()?, "long"),
            None => self.primitive_value::<i64>(),
        }
    }

    fn read_float(&mut self, _schema: &Schema<'_>) -> Result<f32, SerdeError> {
        match self.cursor {
            Some(_) => parse_primitive::<f32>(&self.next_text()?, "float"),
            None => self.primitive_value::<f32>(),
        }
    }

    fn read_double(&mut self, _schema: &Schema<'_>) -> Result<f64, SerdeError> {
        match self.cursor {
            Some(_) => parse_primitive::<f64>(&self.next_text()?, "double"),
            None => self.primitive_value::<f64>(),
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
        // `@mediaType` on a header-bound string travels base64-encoded.
        let media_typed = schema.media_type().is_some() || self.member.media_type().is_some();
        let raw = match self.cursor {
            Some(_) => self.next_text()?,
            // Like legacy, a scalar `@mediaType` string is tokenized as a list
            // (`read_many_from_str`: quote-aware, so `"eyJ..."` is unquoted) and
            // must be exactly one item.
            None if media_typed => {
                let mut tokens = aws_smithy_http::header::read_many_from_str::<String>(self.values.iter().copied())
                    .map_err(|e| SerdeError::invalid_input(e.to_string()))?;
                if tokens.len() != 1 {
                    return Err(SerdeError::invalid_input(format!(
                        "expected one item but found {}",
                        tokens.len()
                    )));
                }
                tokens.remove(0)
            }
            // Other scalar strings use the full single value (no comma splitting),
            // trimmed — matching `one_or_none::<String>`.
            None => self.single_value()?.trim().to_string(),
        };
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
        Err(SerdeError::unsupported("blobs cannot be bound to headers"))
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
            // Scalar map value: first occurrence wins.
            None => self
                .current_values()?
                .first()
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

// Preserve a payload member's XML name when generated deserialization switches to
// the target shape's schema. Only the document root is aliased; child schemas and
// the underlying codec are passed directly to the consumer.
struct StructuredPayloadDeserializer<'a> {
    inner: &'a mut dyn ShapeDeserializer,
    xml_name: &'a str,
}

impl ShapeDeserializer for StructuredPayloadDeserializer<'_> {
    fn read_struct(
        &mut self,
        schema: &Schema<'_>,
        consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let root = Schema::new_struct(schema.shape_id().clone(), schema.shape_type(), schema.members())
            .with_xml_name(self.xml_name);
        self.inner.read_struct(&root, consumer)
    }

    fn read_list(
        &mut self,
        schema: &Schema<'_>,
        consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.inner.read_list(schema, consumer)
    }

    fn read_map(
        &mut self,
        schema: &Schema<'_>,
        consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        self.inner.read_map(schema, consumer)
    }

    unsupported_reads! {
        "expected a structured payload";
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
        read_string -> String,
    }
    fn read_document(&mut self, schema: &Schema<'_>) -> Result<Document, SerdeError> {
        self.inner.read_document(schema)
    }
    fn is_null(&self) -> bool {
        self.inner.is_null()
    }
    fn container_size(&self) -> Option<usize> {
        self.inner.container_size()
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
///
/// Everything it needs beyond the request is read off the input schema handed to `read_struct`:
/// the `@http` URI template for labels, and which members are bound where.
pub(crate) struct RestRequestDeserializer<'a, C> {
    codec: &'a C,
    headers: &'a Headers,
    uri: &'a Uri,
    body: &'a [u8],
}

impl<'a, C: Codec> RestRequestDeserializer<'a, C> {
    pub(crate) fn new(codec: &'a C, uri: &'a Uri, headers: &'a Headers, body: &'a [u8]) -> Self {
        Self {
            codec,
            headers,
            uri,
            body,
        }
    }

    // Header values are valid UTF-8 by construction: the runtime-api `Headers` rejects
    // non-UTF-8 values at `Request::try_from`, exactly where the generated deserializers do.
    fn header_values(&self, name: &str) -> Vec<&'a str> {
        self.headers.get_all(name).collect()
    }
}

impl<C: Codec> ShapeDeserializer for RestRequestDeserializer<'_, C> {
    fn read_struct(
        &mut self,
        schema: &Schema<'_>,
        consumer: &mut dyn FnMut(&Schema<'_>, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        // Labels and query pairs are parsed once, up front.
        let labels: Vec<(Cow<'_, str>, String)> = if schema.members().iter().any(|m| m.http_label().is_some()) {
            let template = schema.http().map(|http| http.uri()).ok_or_else(|| {
                SerdeError::invalid_input("input has @httpLabel members but the operation has no @http trait")
            })?;
            extract_labels(template, self.uri.path())?
                .into_iter()
                .map(|(name, value)| (Cow::Borrowed(name), value))
                .collect()
        } else {
            Vec::new()
        };
        let needs_query = schema
            .members()
            .iter()
            .any(|m| m.http_query().is_some() || m.http_query_params().is_some());
        let query_pairs: Vec<(String, String)> = if needs_query {
            parse_query_pairs(self.uri.query())
        } else {
            Vec::new()
        };

        for member in schema.members() {
            let member_name = member.member_name().unwrap_or_default();
            if member.http_label().is_some() {
                let value = labels
                    .iter()
                    .find(|(name, _)| name.as_ref() == member_name)
                    .map(|(_, raw)| label_value(member, raw))
                    .ok_or_else(|| {
                        SerdeError::invalid_input(format!("no `{{{member_name}}}` label in the `@http` URI pattern"))
                    })??;
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
                // Grouped in order of first appearance. The index keeps this linear: a search of
                // `entries` per pair made many distinct keys quadratic.
                let mut entries: Vec<(String, Vec<String>)> = Vec::new();
                let mut index: HashMap<&str, usize> = HashMap::new();
                for (k, v) in &query_pairs {
                    match index.entry(k.as_str()) {
                        Entry::Occupied(slot) => entries[*slot.get()].1.push(v.clone()),
                        Entry::Vacant(slot) => {
                            slot.insert(entries.len());
                            entries.push((k.clone(), vec![v.clone()]));
                        }
                    }
                }
                let mut deser = StringMapDeserializer::new(entries);
                consumer(member, &mut deser)?;
            } else if let Some(header) = member.http_header() {
                let values = self.header_values(header.value());
                if !values.is_empty() {
                    // Legacy parses these through `read_many_*` / `many_dates` and leaves the
                    // member unset when that yields no tokens (e.g. `x-list:`). A `@mediaType`
                    // string takes the same path, unlike a plain string (`one_or_none`).
                    let list_element = match member.shape_type() {
                        ShapeType::List => member.member(),
                        _ => None,
                    };
                    let date_element = match list_element {
                        Some(element) if element.shape_type() == ShapeType::Timestamp => Some(element),
                        None if member.shape_type() == ShapeType::Timestamp => Some(*member),
                        _ => None,
                    };
                    let text_tokens = match member.shape_type() {
                        ShapeType::Boolean
                        | ShapeType::Byte
                        | ShapeType::Short
                        | ShapeType::Integer
                        | ShapeType::Long
                        | ShapeType::Float
                        | ShapeType::Double => true,
                        ShapeType::String => member.media_type().is_some(),
                        ShapeType::List => date_element.is_none(),
                        _ => false,
                    };
                    if text_tokens {
                        let tokens = aws_smithy_http::header::read_many_from_str::<String>(values.iter().copied())
                            .map_err(|e| SerdeError::invalid_input(e.to_string()))?;
                        if tokens.is_empty() {
                            continue;
                        }
                    }
                    if let Some(element) = date_element {
                        let format = resolve_timestamp_format(element, member, BindingLocation::Header);
                        if aws_smithy_http::header::many_dates(values.iter().copied(), format)
                            .map_err(|e| SerdeError::invalid_input(e.to_string()))?
                            .is_empty()
                        {
                            continue;
                        }
                    }
                    let mut deser = HeaderValuesDeserializer::new(values, member);
                    consumer(member, &mut deser)?;
                }
            } else if let Some(prefix) = member.http_prefix_headers() {
                let prefix = prefix.value();
                // Each header name once, in order of first appearance (`Headers::iter` yields
                // one entry per value).
                let mut names: Vec<&str> = Vec::new();
                let mut seen: HashSet<&str> = HashSet::new();
                for (name, _) in self.headers.iter() {
                    if seen.insert(name) {
                        names.push(name);
                    }
                }
                let mut entries: Vec<(String, Vec<String>)> = Vec::new();
                for (suffix, full_name) in aws_smithy_http::header::headers_for_prefix(names.into_iter(), prefix) {
                    let values: Vec<String> = self
                        .header_values(full_name)
                        .into_iter()
                        .map(|v| v.to_string())
                        .collect();
                    if values.len() > 1 && !member.member().is_some_and(|v| v.shape_type() == ShapeType::List) {
                        return Err(SerdeError::invalid_input(
                            "expected a single prefix header value but found multiple",
                        ));
                    }
                    if !values.is_empty() {
                        entries.push((suffix.to_string(), values));
                    }
                }
                // Always set, even with no matching header: legacy servers yield `Some({})`.
                let mut deser = StringMapDeserializer::new(entries);
                consumer(member, &mut deser)?;
            } else if member.http_payload().is_some() {
                // A streaming payload is never collected: the generated streaming glue attaches
                // the live body after the walker has run.
                if member.streaming() {
                    continue;
                }
                match member.shape_type() {
                    ShapeType::Blob | ShapeType::String => {
                        // Matching `HttpBindingGenerator.kt`: an empty
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
                            // The generated member consumer reads `null` as unset, which is right
                            // for a body member but not for a whole structure/union payload:
                            // legacy rejects it. A `null` document payload stays a value.
                            if matches!(member.shape_type(), ShapeType::Structure | ShapeType::Union) && deser.is_null()
                            {
                                return Err(SerdeError::invalid_input("expected payload member value"));
                            }
                            if let Some(name) = member.xml_name() {
                                let mut payload = StructuredPayloadDeserializer {
                                    inner: &mut deser,
                                    xml_name: name.value(),
                                };
                                consumer(member, &mut payload)?;
                            } else {
                                consumer(member, &mut deser)?;
                            }
                        }
                    }
                }
            }
        }

        // Unbound members come from the codec body document, in wire order.
        let has_unbound_members = schema
            .members()
            .iter()
            .any(|m| m.http_payload().is_none() && is_body_member(m));
        if has_unbound_members && !self.body.is_empty() {
            let mut body_deser = self.codec.create_deserializer(self.body);
            // Resolve only document members. Passing the full input schema lets a body key
            // overwrite a trusted header/label/query binding, or fail parsing its ignored type.
            let members: Vec<_> = schema
                .members()
                .iter()
                .copied()
                .filter(|m| m.http_payload().is_none() && is_body_member(m))
                .collect();
            let mut body_schema = Schema::new_struct(schema.shape_id().clone(), schema.shape_type(), &members);
            if let Some(name) = schema.original_name() {
                body_schema = body_schema.with_original_name(name);
            }
            if let Some(name) = schema.xml_name() {
                body_schema = body_schema.with_xml_name(name.value());
            }
            body_deser.read_struct(&body_schema, consumer)?;
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

        // Query pairs decode in order with form-urlencoded semantics, exactly
        // as legacy generated deserializers do: a key without `=` gets "",
        // `+` decodes to a space, and invalid UTF-8 decodes lossily instead
        // of rejecting the request.
        assert_eq!(
            parse_query_pairs(Some("a=1&b=x%20y&flag&c=&d=x+y&e=%FF")),
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "x y".to_string()),
                ("flag".to_string(), "".to_string()),
                ("c".to_string(), "".to_string()),
                ("d".to_string(), "x y".to_string()),
                ("e".to_string(), "\u{FFFD}".to_string()),
            ]
        );
        assert!(parse_query_pairs(None).is_empty());

        // Labels: plain (raw; decoded per member by `label_value`), greedy (keeps slashes),
        // greedy with a literal suffix, query-literal templates ignored, mismatches error.
        assert_eq!(
            extract_labels("/pets/{name}/{age}", "/pets/rex%20jr/7").unwrap(),
            vec![("name", "rex%20jr".to_string()), ("age", "7".to_string())]
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
    // The operation's `@http` trait is transcribed onto the input schema by codegen.
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

    fn collect(uri: &Uri, headers: &Headers, body: &[u8]) -> Result<Collected, SerdeError> {
        let codec = json_codec();
        let mut deser = RestRequestDeserializer::new(&codec, uri, headers, body);
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

    fn request_parts(uri: &str, headers: &[(&'static str, &str)]) -> (Uri, Headers) {
        let mut builder = http::Request::builder().method("POST").uri(uri);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request =
            aws_smithy_runtime_api::http::Request::try_from(builder.body(()).unwrap()).expect("valid test request");
        let parts = request.into_parts();
        (parts.uri, parts.headers)
    }

    static QUERY_PARAMS_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#QpInput$params", "test", "QpInput"),
        ShapeType::Map,
        "params",
        0,
    )
    .with_http_query_params();
    static QUERY_PARAMS_MEMBERS: [&Schema<'static>; 1] = [&QUERY_PARAMS_MEMBER];
    static QUERY_PARAMS_INPUT: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#QpInput", "test", "QpInput"),
        ShapeType::Structure,
        &QUERY_PARAMS_MEMBERS,
    )
    .with_http(HttpTrait::new("GET", "/qp", Some(200)));

    /// `@httpQueryParams` groups every value per key, keys in order of first appearance.
    #[test]
    fn query_params_group_values_in_first_seen_order() {
        let codec = json_codec();
        let collect_params = |uri: &str| {
            let (uri, headers) = request_parts(uri, &[]);
            let mut deser = RestRequestDeserializer::new(&codec, &uri, &headers, b"");
            let mut params: Vec<(String, Vec<String>)> = Vec::new();
            deser
                .read_struct(&QUERY_PARAMS_INPUT, &mut |member, d| {
                    d.read_map(member, &mut |key, d| {
                        let mut values = Vec::new();
                        d.read_list(member, &mut |element| {
                            values.push(element.read_string(member)?);
                            Ok(())
                        })?;
                        params.push((key, values));
                        Ok(())
                    })
                })
                .unwrap();
            params
        };
        let owned = |pairs: &[(&str, &[&str])]| -> Vec<(String, Vec<String>)> {
            pairs
                .iter()
                .map(|(k, vs)| (k.to_string(), vs.iter().map(|v| v.to_string()).collect()))
                .collect()
        };
        assert_eq!(
            collect_params("/qp?b=1&a=2&b=3&c=&a=4"),
            owned(&[("b", &["1", "3"]), ("a", &["2", "4"]), ("c", &[""])])
        );
        // Many distinct keys keep their order (and stay linear to group).
        let uri = format!(
            "/qp?{}",
            (0..5000).map(|i| format!("k{i}={i}")).collect::<Vec<_>>().join("&")
        );
        let params = collect_params(&uri);
        assert_eq!(params.len(), 5000);
        assert!(params
            .iter()
            .enumerate()
            .all(|(i, (k, vs))| *k == format!("k{i}") && *vs == [i.to_string()]));
    }

    #[test]
    fn composite_deserializer() {
        // Every binding location routes: label, query scalar + list, header,
        // prefix headers, body member via the codec.
        let (uri, headers) = request_parts(
            "/pets/rex?age=7&tag=a&tag=b",
            &[("x-token", "secret"), ("x-meta-color", "red"), ("x-meta-size", "xl")],
        );
        let out = collect(&uri, &headers, br#"{"note":"hello"}"#).unwrap();
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
        let (uri, headers) = request_parts("/pets/rex", &[]);
        let out = collect(&uri, &headers, b"").unwrap();
        assert_eq!(out.name.as_deref(), Some("rex"));
        assert_eq!((out.age, out.token, out.note), (None, None, None));
        assert!(out.tags.is_empty());

        // Scalar query members: first occurrence wins.
        let (uri, headers) = request_parts("/pets/rex?age=1&age=2", &[]);
        assert_eq!(collect(&uri, &headers, b"").unwrap().age, Some(1));

        // Unparseable bound values are wire-level errors.
        let (uri, headers) = request_parts("/pets/rex?age=notanumber", &[]);
        assert!(collect(&uri, &headers, b"").is_err());
    }
    static HP_STRING: Schema<'static> = Schema::new(
        ShapeId::from_parts("smithy.api#String", "smithy.api", "String"),
        ShapeType::String,
    );
    static HP_TIMESTAMP: Schema<'static> = Schema::new(
        ShapeId::from_parts("smithy.api#Timestamp", "smithy.api", "Timestamp"),
        ShapeType::Timestamp,
    );
    static HP_LIST_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#HpInput$list", "test", "HpInput"),
        ShapeType::List,
        "list",
        0,
    )
    .with_list_member(&HP_STRING)
    .with_http_header("x-list");
    static HP_TIMESTAMPS_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#HpInput$timestamps", "test", "HpInput"),
        ShapeType::List,
        "timestamps",
        1,
    )
    .with_list_member(&HP_TIMESTAMP)
    .with_http_header("x-tl");
    static HP_JSON_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#HpInput$json", "test", "HpInput"),
        ShapeType::String,
        "json",
        2,
    )
    .with_media_type("application/json")
    .with_http_header("x-json");
    static HP_PREFIX_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#HpInput$prefix", "test", "HpInput"),
        ShapeType::Map,
        "prefix",
        3,
    )
    .with_http_prefix_headers("x-foo-");
    static HP_MEMBERS: [&Schema<'static>; 4] = [
        &HP_LIST_MEMBER,
        &HP_TIMESTAMPS_MEMBER,
        &HP_JSON_MEMBER,
        &HP_PREFIX_MEMBER,
    ];
    static HP_INPUT: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#HpInput", "test", "HpInput"),
        ShapeType::Structure,
        &HP_MEMBERS,
    )
    .with_http(HttpTrait::new("GET", "/hp", Some(200)));

    /// Which header-bound members get set, and to how many list items / map entries (or the
    /// string value). Legacy leaves a list or `@mediaType` header with no tokens unset, and
    /// always sets `@httpPrefixHeaders` (possibly empty).
    fn header_presence(headers: &[(&'static str, &str)]) -> Vec<(String, String)> {
        let codec = json_codec();
        let (uri, headers) = request_parts("/hp", headers);
        let mut deser = RestRequestDeserializer::new(&codec, &uri, &headers, b"");
        let mut set = Vec::new();
        deser
            .read_struct(&HP_INPUT, &mut |member, d| {
                let summary = match member.member_index() {
                    Some(0) | Some(1) => {
                        let mut n = 0;
                        d.read_list(member, &mut |element| {
                            match member.member().map(|m| m.shape_type()) {
                                Some(ShapeType::Timestamp) => {
                                    element.read_timestamp(member.member().unwrap())?;
                                }
                                _ => {
                                    element.read_string(member)?;
                                }
                            }
                            n += 1;
                            Ok(())
                        })?;
                        n.to_string()
                    }
                    Some(2) => d.read_string(member)?,
                    Some(3) => {
                        let mut n = 0;
                        d.read_map(member, &mut |_, value| {
                            value.read_string(member)?;
                            n += 1;
                            Ok(())
                        })?;
                        n.to_string()
                    }
                    _ => return Ok(()),
                };
                set.push((member.member_name().unwrap().to_string(), summary));
                Ok(())
            })
            .unwrap();
        set
    }

    #[test]
    fn empty_header_presence_matches_legacy() {
        let owned = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
        };
        // No headers: only the prefix map is set (empty).
        assert_eq!(header_presence(&[]), owned(&[("prefix", "0")]));
        // Empty list and `@mediaType` headers leave the member unset.
        assert_eq!(
            header_presence(&[("x-list", ""), ("x-tl", ""), ("x-json", "")]),
            owned(&[("prefix", "0")])
        );
        // Non-empty values are still read.
        assert_eq!(
            header_presence(&[
                ("x-list", "a, b"),
                ("x-tl", "Thu, 01 Jan 1970 00:00:00 GMT"),
                ("x-json", "e30="),
                ("x-foo-a", ""),
            ]),
            owned(&[("list", "2"), ("timestamps", "1"), ("json", "{}"), ("prefix", "1")])
        );
    }

    static SP_TARGET_MEMBERS: [&Schema<'static>; 0] = [];
    static SP_STRUCT_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#SpInput$nested", "test", "SpInput"),
        ShapeType::Structure,
        "nested",
        0,
    )
    .with_http_payload();
    static SP_DOC_MEMBER: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("test#SpDocInput$doc", "test", "SpDocInput"),
        ShapeType::Document,
        "doc",
        0,
    )
    .with_http_payload();
    static SP_MEMBERS: [&Schema<'static>; 1] = [&SP_STRUCT_MEMBER];
    static SP_DOC_MEMBERS: [&Schema<'static>; 1] = [&SP_DOC_MEMBER];
    static SP_INPUT: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#SpInput", "test", "SpInput"),
        ShapeType::Structure,
        &SP_MEMBERS,
    )
    .with_http(HttpTrait::new("POST", "/sp", Some(200)));
    static SP_DOC_INPUT: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#SpDocInput", "test", "SpDocInput"),
        ShapeType::Structure,
        &SP_DOC_MEMBERS,
    )
    .with_http(HttpTrait::new("POST", "/sp", Some(200)));
    static SP_TARGET: Schema<'static> = Schema::new_struct(
        ShapeId::from_parts("test#Nested", "test", "Nested"),
        ShapeType::Structure,
        &SP_TARGET_MEMBERS,
    );

    /// Reads a payload the way generated code does (`null` → unset). `Ok(Some(true))` = set.
    fn read_payload(input: &Schema<'_>, body: &[u8]) -> Result<Option<bool>, SerdeError> {
        let codec = json_codec();
        let (uri, headers) = request_parts("/sp", &[]);
        let mut deser = RestRequestDeserializer::new(&codec, &uri, &headers, body);
        let mut set = None;
        deser.read_struct(input, &mut |member, d| {
            if d.is_null() {
                d.read_null()?;
            } else if member.shape_type() == ShapeType::Document {
                d.read_document(member)?;
                set = Some(true);
            } else {
                d.read_struct(&SP_TARGET, &mut |_, _| Ok(()))?;
                set = Some(true);
            }
            Ok(())
        })?;
        Ok(set)
    }

    /// Legacy rejects a structure/union `@httpPayload` whose body is `null` ("expected payload
    /// member value"); a `null` document payload is a value, and an empty body leaves it unset.
    #[test]
    fn null_struct_payload_is_rejected_like_legacy() {
        for body in [&b"null"[..], b" null", b"\n null \n"] {
            assert!(
                read_payload(&SP_INPUT, body).is_err(),
                "{:?}",
                std::str::from_utf8(body)
            );
        }
        assert_eq!(read_payload(&SP_INPUT, b"").unwrap(), None);
        assert_eq!(read_payload(&SP_INPUT, b"{}").unwrap(), Some(true));
        assert!(read_payload(&SP_DOC_INPUT, b"null").is_ok());
    }

    /// Legacy percent-decodes string and timestamp labels but parses number and boolean labels
    /// from the raw segment: `/pets/%37` is the name "7", but `%37` is not the age 7.
    #[test]
    fn only_string_and_timestamp_labels_are_percent_decoded() {
        assert_eq!(label_value(&NAME_MEMBER, "rex%20jr").unwrap(), "rex jr");
        assert_eq!(label_value(&NAME_MEMBER, "%37").unwrap(), "7");
        assert!(label_value(&NAME_MEMBER, "%FF").is_err());
        let ts = Schema::new_member(
            ShapeId::from_parts("test#Input$ts", "test", "Input"),
            ShapeType::Timestamp,
            "ts",
            0,
        );
        assert_eq!(
            label_value(&ts, "2020-01-01T00%3A00%3A00Z").unwrap(),
            "2020-01-01T00:00:00Z"
        );
        for raw in ["%37", "%2D1", "tru%65"] {
            assert_eq!(label_value(&AGE_MEMBER, raw).unwrap(), raw);
        }

        // End to end: a string label decodes; an integer label is parsed raw and rejected.
        let (uri, headers) = request_parts("/pets/%37", &[]);
        assert_eq!(collect(&uri, &headers, b"").unwrap().name.as_deref(), Some("7"));
        let mut deser = DecodedValuesDeserializer::new(
            vec![Cow::Owned(label_value(&AGE_MEMBER, "%37").unwrap())],
            &AGE_MEMBER,
            BindingLocation::Label,
        );
        assert!(deser.read_integer(&AGE_MEMBER).is_err());
    }

    /// A scalar `@mediaType` header is tokenized like legacy's `read_many_from_str`: a quoted
    /// value is unquoted before base64 decoding, and more than one item is rejected.
    #[test]
    fn media_type_header_is_tokenized_like_legacy() {
        let read =
            |values: Vec<&str>| HeaderValuesDeserializer::new(values, &HP_JSON_MEMBER).read_string(&HP_JSON_MEMBER);
        assert_eq!(read(vec!["e30="]).unwrap(), "{}");
        assert_eq!(read(vec!["\"e30=\""]).unwrap(), "{}");
        for values in [vec!["e30=, e30="], vec!["e30=", "e30="], vec!["\"e3,0=\""]] {
            assert!(read(values.clone()).is_err(), "{values:?}");
        }
        // A plain string header keeps the whole value, quotes and commas included.
        assert_eq!(
            HeaderValuesDeserializer::new(vec!["\"a, b\""], &TOKEN_MEMBER)
                .read_string(&TOKEN_MEMBER)
                .unwrap(),
            "\"a, b\""
        );
    }

    #[test]
    fn prefix_repeats_reject_but_scalar_query_keeps_first() {
        let (uri, headers) = request_parts("/pets/rex", &[("x-meta-color", "red"), ("x-meta-color", "blue")]);
        assert!(collect(&uri, &headers, b"").is_err());
        let (uri, headers) = request_parts("/pets/rex?age=1&age=2", &[("x-token", "")]);
        let out = collect(&uri, &headers, b"").unwrap();
        assert_eq!(out.age, Some(1));
        assert_eq!(out.token.as_deref(), Some(""));
    }

    #[test]
    fn label_and_query_primitives_are_not_trimmed() {
        // Legacy parses the percent-decoded segment as-is: " 1" is an error, not 1.
        for location in [BindingLocation::Label, BindingLocation::Query] {
            for value in [" 1", "1 ", "\t1"] {
                let mut deser = DecodedValuesDeserializer::new(vec![value.into()], &AGE_MEMBER, location);
                assert!(deser.read_integer(&AGE_MEMBER).is_err(), "{location:?} {value:?}");
            }
            let mut deser = DecodedValuesDeserializer::new(vec!["1".into()], &AGE_MEMBER, location);
            assert_eq!(deser.read_integer(&AGE_MEMBER).unwrap(), 1);
        }
    }

    #[test]
    fn header_primitives_are_still_trimmed() {
        // Headers keep legacy `read_many` semantics: surrounding whitespace is ignored.
        for values in [vec![" 7 "], vec!["\t7"], vec![" 7, 8 "]] {
            let mut deser = HeaderValuesDeserializer::new(values, &TAGS_MEMBER);
            let mut seen = vec![];
            deser
                .read_list(&TAGS_MEMBER, &mut |element| {
                    seen.push(element.read_integer(&AGE_MEMBER)?);
                    Ok(())
                })
                .unwrap();
            assert!(seen.iter().all(|n| *n == 7 || *n == 8), "{seen:?}");
        }
        assert_eq!(
            HeaderValuesDeserializer::new(vec![" 7 "], &AGE_MEMBER)
                .read_integer(&AGE_MEMBER)
                .unwrap(),
            7
        );
    }

    #[test]
    fn primitive_header_tokenization() {
        for values in [vec!["7,"], vec!["", "7"], vec!["\"7\""]] {
            assert_eq!(
                HeaderValuesDeserializer::new(values, &AGE_MEMBER)
                    .read_integer(&AGE_MEMBER)
                    .unwrap(),
                7
            );
        }
        for values in [vec!["7", "8"], vec!["7,8"], vec![" "], vec![r#"" 7 ""#]] {
            assert!(HeaderValuesDeserializer::new(values, &AGE_MEMBER)
                .read_integer(&AGE_MEMBER)
                .is_err());
        }
        let mut list = HeaderValuesDeserializer::new(vec!["7,8", "9"], &TAGS_MEMBER);
        let mut seen = Vec::new();
        list.read_list(&TAGS_MEMBER, &mut |d| {
            seen.push(d.read_integer(&AGE_MEMBER)?);
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, [7, 8, 9]);
    }
    #[test]
    fn body_cannot_override_http_bindings_or_parse_their_types() {
        let (uri, headers) = request_parts(
            "/pets/rex?age=7&tag=real",
            &[("x-token", "trusted"), ("x-meta-color", "red")],
        );
        for body in [
            br#"{"name":"evil","age":99,"tags":["evil"],"token":"evil","meta":{"color":"evil"},"note":"body"}"#
                .as_slice(),
            br#"{"name":false,"age":{},"tags":true,"token":12,"meta":[],"note":"body"}"#,
        ] {
            let out = collect(&uri, &headers, body).unwrap();
            assert_eq!(out.name.as_deref(), Some("rex"));
            assert_eq!(out.age, Some(7));
            assert_eq!(out.tags, ["real"]);
            assert_eq!(out.token.as_deref(), Some("trusted"));
            assert_eq!(out.meta, [("color".to_owned(), "red".to_owned())]);
            assert_eq!(out.note.as_deref(), Some("body"));
        }
        // Excluded bindings are still parsed as unknown values, validating JSON syntax.
        assert!(collect(&uri, &headers, br#"{"token":[1,]}"#).is_err());
    }

    #[test]
    fn greedy_labels_preserve_empty_segments() {
        for (path, expected) in [("/data/", ""), ("/data/a//b", "a//b"), ("/data//a/", "/a/")] {
            assert_eq!(
                extract_labels("/data/{key+}", path).unwrap(),
                [("key", expected.to_owned())]
            );
        }
        assert_eq!(
            extract_labels("/data/{key+}/meta", "/data//meta").unwrap(),
            [("key", "".to_owned())]
        );
        assert!(extract_labels("/data/{key+}", "/data").is_err());
    }
}
