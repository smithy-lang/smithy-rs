use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
use aws_smithy_schema::{Schema, ShapeType};
use aws_smithy_types::date_time::Format;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};
use std::borrow::Cow;

#[derive(Copy, Clone, Debug)]
enum BindingLocation {
    Header,
}

fn resolve_timestamp_format(
    read_schema: &Schema<'_>,
    member: &Schema<'_>,
    location: BindingLocation,
) -> Format {
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
        },
    }
}

fn parse_primitive<T: aws_smithy_types::primitive::Parse>(
    value: &str,
    what: &str,
) -> Result<T, SerdeError> {
    T::parse_smithy_primitive(value.trim())
        .map_err(|err| SerdeError::invalid_input(format!("invalid {what}: {err}")))
}

pub(crate) struct HeaderValuesDeserializer<'a> {
    values: HeaderValues<'a>,
    member: &'a Schema<'a>,
    current: Option<HeaderToken<'a>>,
    list_size: Option<usize>,
}

enum HeaderValues<'a> {
    One(&'a str),
    Many(Vec<&'a str>),
}

impl<'a> HeaderValues<'a> {
    fn len(&self) -> usize {
        match self {
            Self::One(_) => 1,
            Self::Many(values) => values.len(),
        }
    }

    fn get(&self, idx: usize) -> Option<&'a str> {
        match self {
            Self::One(value) if idx == 0 => Some(*value),
            Self::One(_) => None,
            Self::Many(values) => values.get(idx).copied(),
        }
    }

    fn single(&self) -> Option<&'a str> {
        match self {
            Self::One(value) => Some(*value),
            Self::Many(values) if values.len() == 1 => values.first().copied(),
            Self::Many(_) => None,
        }
    }
}

enum HeaderToken<'a> {
    Text(Cow<'a, str>),
    Date(DateTime),
}

impl<'a> HeaderValuesDeserializer<'a> {
    pub(crate) fn new(member: &'a Schema<'a>, values: Vec<&'a str>) -> Self {
        debug_assert!(!values.is_empty());
        let values = if values.len() == 1 {
            HeaderValues::One(values[0])
        } else {
            HeaderValues::Many(values)
        };
        Self::from_values(member, values)
    }

    pub(crate) fn one(member: &'a Schema<'a>, value: &'a str) -> Self {
        Self::from_values(member, HeaderValues::One(value))
    }

    fn from_values(member: &'a Schema<'a>, values: HeaderValues<'a>) -> Self {
        Self {
            values,
            member,
            current: None,
            list_size: None,
        }
    }

    fn next_token(&mut self) -> Result<HeaderToken<'a>, SerdeError> {
        self.current
            .take()
            .ok_or_else(|| SerdeError::invalid_input("header list element read outside a list"))
    }

    fn next_text(&mut self) -> Result<Cow<'a, str>, SerdeError> {
        match self.next_token()? {
            HeaderToken::Text(s) => Ok(s),
            HeaderToken::Date(_) => Err(SerdeError::invalid_input(
                "expected a string header element, found a timestamp",
            )),
        }
    }

    fn single_value(&self) -> Result<&'a str, SerdeError> {
        self.values.single().ok_or_else(|| {
            SerdeError::invalid_input("expected a single header value but found multiple")
        })
    }
}

fn many_dates(values: &HeaderValues<'_>, format: Format) -> Result<Vec<DateTime>, SerdeError> {
    match values {
        HeaderValues::One(value) => {
            aws_smithy_http::header::many_dates(std::iter::once(*value), format)
        }
        HeaderValues::Many(values) => {
            aws_smithy_http::header::many_dates(values.iter().copied(), format)
        }
    }
    .map_err(|err| SerdeError::invalid_input(format!("{err}")))
}

pub(crate) struct ScalarHeaderValueDeserializer<'a> {
    value: &'a str,
    member: &'a Schema<'a>,
}

impl<'a> ScalarHeaderValueDeserializer<'a> {
    pub(crate) fn new(member: &'a Schema<'a>, value: &'a str) -> Self {
        Self { value, member }
    }

    fn value(&self) -> &'a str {
        self.value.trim()
    }
}

pub(crate) struct PrefixHeadersDeserializer<'a> {
    member: &'a Schema<'a>,
    values: PrefixHeaderValues<'a>,
}

enum PrefixHeaderValues<'a> {
    #[allow(dead_code)]
    HeaderMap {
        prefix: &'a str,
        headers: &'a http::HeaderMap,
    },
    #[cfg(test)]
    Entries(Vec<(String, &'a str)>),
}

impl<'a> PrefixHeadersDeserializer<'a> {
    #[cfg(test)]
    pub(crate) fn new(member: &'a Schema<'a>, entries: Vec<(String, &'a str)>) -> Self {
        Self {
            member,
            values: PrefixHeaderValues::Entries(entries),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn from_headers(
        member: &'a Schema<'a>,
        prefix: &'a str,
        headers: &'a http::HeaderMap,
    ) -> Self {
        Self {
            member,
            values: PrefixHeaderValues::HeaderMap { prefix, headers },
        }
    }

    #[allow(dead_code)]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn len(&self) -> usize {
        match &self.values {
            PrefixHeaderValues::HeaderMap { prefix, headers } => headers
                .iter()
                .filter(|(name, _)| header_name_has_prefix(name.as_str(), prefix))
                .count(),
            #[cfg(test)]
            PrefixHeaderValues::Entries(entries) => entries.len(),
        }
    }
}

fn header_name_has_prefix(name: &str, prefix: &str) -> bool {
    name.len() >= prefix.len() && name[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn trim_cow(value: Cow<'_, str>) -> Cow<'_, str> {
    match value {
        Cow::Owned(value) => Cow::Owned(value.trim().to_owned()),
        Cow::Borrowed(value) => Cow::Borrowed(value.trim()),
    }
}

fn replace_cow<'a>(value: Cow<'a, str>, pattern: &str, replacement: &str) -> Cow<'a, str> {
    if value.contains(pattern) {
        Cow::Owned(value.replace(pattern, replacement))
    } else {
        value
    }
}

fn read_header_value(input: &[u8]) -> Result<(Cow<'_, str>, &[u8]), SerdeError> {
    for (index, &byte) in input.iter().enumerate() {
        let current_slice = &input[index..];
        match byte {
            b' ' | b'\t' => {}
            b'"' => return read_quoted_header_value(&current_slice[1..]),
            _ => {
                let (value, rest) = read_unquoted_header_value(current_slice)?;
                return Ok((trim_cow(value), rest));
            }
        }
    }

    Ok((Cow::Borrowed(""), &[]))
}

fn read_unquoted_header_value(input: &[u8]) -> Result<(Cow<'_, str>, &[u8]), SerdeError> {
    let next_delim = input.iter().position(|&b| b == b',').unwrap_or(input.len());
    let (first, next) = input.split_at(next_delim);
    let first = std::str::from_utf8(first)
        .map_err(|_| SerdeError::invalid_input("header was not valid utf-8"))?;
    Ok((Cow::Borrowed(first), then_comma(next)?))
}

fn read_quoted_header_value(input: &[u8]) -> Result<(Cow<'_, str>, &[u8]), SerdeError> {
    for index in 0..input.len() {
        match input[index] {
            b'"' if index == 0 || input[index - 1] != b'\\' => {
                let mut inner = Cow::Borrowed(
                    std::str::from_utf8(&input[0..index])
                        .map_err(|_| SerdeError::invalid_input("header was not valid utf-8"))?,
                );
                inner = replace_cow(inner, "\\\"", "\"");
                inner = replace_cow(inner, "\\\\", "\\");
                let rest = then_comma(&input[(index + 1)..])?;
                return Ok((inner, rest));
            }
            _ => {}
        }
    }
    Err(SerdeError::invalid_input(
        "header value had quoted value without end quote",
    ))
}

fn then_comma(s: &[u8]) -> Result<&[u8], SerdeError> {
    if s.is_empty() {
        Ok(s)
    } else if s.starts_with(b",") {
        Ok(&s[1..])
    } else {
        Err(SerdeError::invalid_input("expected delimiter `,`"))
    }
}

impl ShapeDeserializer for ScalarHeaderValueDeserializer<'_> {
    fn read_struct(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(
            &Schema<'_>,
            &mut dyn ShapeDeserializer,
        ) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "structures cannot be bound to headers",
        ))
    }

    fn read_list(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "lists must use the header list deserializer",
        ))
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
        parse_primitive::<bool>(self.value(), "boolean")
    }

    fn read_byte(&mut self, _schema: &Schema<'_>) -> Result<i8, SerdeError> {
        parse_primitive::<i8>(self.value(), "byte")
    }

    fn read_short(&mut self, _schema: &Schema<'_>) -> Result<i16, SerdeError> {
        parse_primitive::<i16>(self.value(), "short")
    }

    fn read_integer(&mut self, _schema: &Schema<'_>) -> Result<i32, SerdeError> {
        parse_primitive::<i32>(self.value(), "integer")
    }

    fn read_long(&mut self, _schema: &Schema<'_>) -> Result<i64, SerdeError> {
        parse_primitive::<i64>(self.value(), "long")
    }

    fn read_float(&mut self, _schema: &Schema<'_>) -> Result<f32, SerdeError> {
        parse_primitive::<f32>(self.value(), "float")
    }

    fn read_double(&mut self, _schema: &Schema<'_>) -> Result<f64, SerdeError> {
        parse_primitive::<f64>(self.value(), "double")
    }

    fn read_big_integer(&mut self, _schema: &Schema<'_>) -> Result<BigInteger, SerdeError> {
        use std::str::FromStr;
        let v = self.value();
        BigInteger::from_str(v)
            .map_err(|_| SerdeError::invalid_input(format!("invalid big integer: {v}")))
    }

    fn read_big_decimal(&mut self, _schema: &Schema<'_>) -> Result<BigDecimal, SerdeError> {
        use std::str::FromStr;
        let v = self.value();
        BigDecimal::from_str(v)
            .map_err(|_| SerdeError::invalid_input(format!("invalid big decimal: {v}")))
    }

    fn read_string(&mut self, schema: &Schema<'_>) -> Result<String, SerdeError> {
        let raw = self.value();
        let media_typed = schema.media_type().is_some() || self.member.media_type().is_some();
        if media_typed {
            let decoded = aws_smithy_types::base64::decode(raw)
                .map_err(|err| SerdeError::invalid_input(format!("invalid base64: {err}")))?;
            String::from_utf8(decoded)
                .map_err(|_| SerdeError::invalid_input("base64-decoded header was not valid UTF-8"))
        } else {
            Ok(raw.to_owned())
        }
    }

    fn read_blob(&mut self, _schema: &Schema<'_>) -> Result<Blob, SerdeError> {
        let v = self.value();
        let decoded = aws_smithy_types::base64::decode(v)
            .map_err(|err| SerdeError::invalid_input(format!("invalid base64: {err}")))?;
        Ok(Blob::new(decoded))
    }

    fn read_timestamp(&mut self, schema: &Schema<'_>) -> Result<DateTime, SerdeError> {
        let format = resolve_timestamp_format(schema, self.member, BindingLocation::Header);
        DateTime::from_str(self.value(), format)
            .map_err(|err| SerdeError::invalid_input(format!("invalid timestamp: {err}")))
    }

    fn read_document(&mut self, _schema: &Schema<'_>) -> Result<Document, SerdeError> {
        Err(SerdeError::unsupported(
            "documents cannot be bound to headers",
        ))
    }

    fn is_null(&self) -> bool {
        false
    }

    fn container_size(&self) -> Option<usize> {
        None
    }
}

impl ShapeDeserializer for PrefixHeadersDeserializer<'_> {
    fn read_struct(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(
            &Schema<'_>,
            &mut dyn ShapeDeserializer,
        ) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "structures cannot be bound to prefix headers",
        ))
    }

    fn read_list(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "lists cannot be bound directly to prefix headers",
        ))
    }

    fn read_map(
        &mut self,
        schema: &Schema<'_>,
        consumer: &mut dyn FnMut(String, &mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let value_schema = schema.member().unwrap_or(self.member);
        match &self.values {
            PrefixHeaderValues::HeaderMap { prefix, headers } => {
                for (name, value) in headers.iter() {
                    let name = name.as_str();
                    if !header_name_has_prefix(name, prefix) {
                        continue;
                    }
                    let value = value.to_str().map_err(|err| {
                        SerdeError::invalid_input(format!(
                            "invalid header value for `{name}`: {err}"
                        ))
                    })?;
                    let mut value_deserializer =
                        ScalarHeaderValueDeserializer::new(value_schema, value);
                    consumer(name[prefix.len()..].to_string(), &mut value_deserializer)?;
                }
            }
            #[cfg(test)]
            PrefixHeaderValues::Entries(entries) => {
                for (key, value) in entries {
                    let mut value_deserializer =
                        ScalarHeaderValueDeserializer::new(value_schema, value);
                    consumer(key.clone(), &mut value_deserializer)?;
                }
            }
        }
        Ok(())
    }

    fn read_boolean(&mut self, _schema: &Schema<'_>) -> Result<bool, SerdeError> {
        Err(SerdeError::unsupported(
            "prefix headers must be read as a map",
        ))
    }

    fn read_byte(&mut self, _schema: &Schema<'_>) -> Result<i8, SerdeError> {
        Err(SerdeError::unsupported(
            "prefix headers must be read as a map",
        ))
    }

    fn read_short(&mut self, _schema: &Schema<'_>) -> Result<i16, SerdeError> {
        Err(SerdeError::unsupported(
            "prefix headers must be read as a map",
        ))
    }

    fn read_integer(&mut self, _schema: &Schema<'_>) -> Result<i32, SerdeError> {
        Err(SerdeError::unsupported(
            "prefix headers must be read as a map",
        ))
    }

    fn read_long(&mut self, _schema: &Schema<'_>) -> Result<i64, SerdeError> {
        Err(SerdeError::unsupported(
            "prefix headers must be read as a map",
        ))
    }

    fn read_float(&mut self, _schema: &Schema<'_>) -> Result<f32, SerdeError> {
        Err(SerdeError::unsupported(
            "prefix headers must be read as a map",
        ))
    }

    fn read_double(&mut self, _schema: &Schema<'_>) -> Result<f64, SerdeError> {
        Err(SerdeError::unsupported(
            "prefix headers must be read as a map",
        ))
    }

    fn read_big_integer(&mut self, _schema: &Schema<'_>) -> Result<BigInteger, SerdeError> {
        Err(SerdeError::unsupported(
            "prefix headers must be read as a map",
        ))
    }

    fn read_big_decimal(&mut self, _schema: &Schema<'_>) -> Result<BigDecimal, SerdeError> {
        Err(SerdeError::unsupported(
            "prefix headers must be read as a map",
        ))
    }

    fn read_string(&mut self, _schema: &Schema<'_>) -> Result<String, SerdeError> {
        Err(SerdeError::unsupported(
            "prefix headers must be read as a map",
        ))
    }

    fn read_blob(&mut self, _schema: &Schema<'_>) -> Result<Blob, SerdeError> {
        Err(SerdeError::unsupported(
            "prefix headers must be read as a map",
        ))
    }

    fn read_timestamp(&mut self, _schema: &Schema<'_>) -> Result<DateTime, SerdeError> {
        Err(SerdeError::unsupported(
            "prefix headers must be read as a map",
        ))
    }

    fn read_document(&mut self, _schema: &Schema<'_>) -> Result<Document, SerdeError> {
        Err(SerdeError::unsupported(
            "prefix headers must be read as a map",
        ))
    }

    fn is_null(&self) -> bool {
        false
    }

    fn container_size(&self) -> Option<usize> {
        Some(self.len())
    }
}

impl ShapeDeserializer for HeaderValuesDeserializer<'_> {
    fn read_struct(
        &mut self,
        _schema: &Schema<'_>,
        _consumer: &mut dyn FnMut(
            &Schema<'_>,
            &mut dyn ShapeDeserializer,
        ) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::unsupported(
            "structures cannot be bound to headers",
        ))
    }

    fn read_list(
        &mut self,
        schema: &Schema<'_>,
        consumer: &mut dyn FnMut(&mut dyn ShapeDeserializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let element = schema.member();
        let element_is_timestamp = element
            .map(|e| e.shape_type() == ShapeType::Timestamp)
            .unwrap_or(false);
        if element_is_timestamp {
            let element = element.expect("checked above");
            let format = resolve_timestamp_format(element, self.member, BindingLocation::Header);
            if format == Format::HttpDate {
                let dates = many_dates(&self.values, format)?;
                self.list_size = Some(dates.len());
                for date in dates {
                    self.current = Some(HeaderToken::Date(date));
                    consumer(self)?;
                }
                self.current = None;
            } else {
                self.list_size = None;
                let values_len = self.values.len();
                for idx in 0..values_len {
                    let header = self.values.get(idx).expect("index checked by len");
                    let mut rest = header.as_bytes();
                    while !rest.is_empty() {
                        let (value, next) = read_header_value(rest)?;
                        let date = DateTime::from_str(&value, format).map_err(|err| {
                            SerdeError::invalid_input(format!("invalid timestamp: {err}"))
                        })?;
                        self.current = Some(HeaderToken::Date(date));
                        consumer(self)?;
                        self.current = None;
                        rest = next;
                    }
                }
            }
        } else {
            self.list_size = None;
            let values_len = self.values.len();
            for idx in 0..values_len {
                let header = self.values.get(idx).expect("index checked by len");
                let mut rest = header.as_bytes();
                while !rest.is_empty() {
                    // `aws_smithy_http::header::read_many_from_str::<String>` has the
                    // right comma/quote semantics, but its public API eagerly allocates
                    // a `Vec<String>`. Keep parsed text borrowed until the consumer asks
                    // for an owned Smithy string.
                    let (value, next) = read_header_value(rest)?;
                    self.current = Some(HeaderToken::Text(value));
                    consumer(self)?;
                    self.current = None;
                    rest = next;
                }
            }
        }
        self.list_size = None;
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
        match self.current {
            Some(_) => parse_primitive::<bool>(&self.next_text()?, "boolean"),
            None => parse_primitive::<bool>(self.single_value()?, "boolean"),
        }
    }

    fn read_byte(&mut self, _schema: &Schema<'_>) -> Result<i8, SerdeError> {
        match self.current {
            Some(_) => parse_primitive::<i8>(&self.next_text()?, "byte"),
            None => parse_primitive::<i8>(self.single_value()?, "byte"),
        }
    }

    fn read_short(&mut self, _schema: &Schema<'_>) -> Result<i16, SerdeError> {
        match self.current {
            Some(_) => parse_primitive::<i16>(&self.next_text()?, "short"),
            None => parse_primitive::<i16>(self.single_value()?, "short"),
        }
    }

    fn read_integer(&mut self, _schema: &Schema<'_>) -> Result<i32, SerdeError> {
        match self.current {
            Some(_) => parse_primitive::<i32>(&self.next_text()?, "integer"),
            None => parse_primitive::<i32>(self.single_value()?, "integer"),
        }
    }

    fn read_long(&mut self, _schema: &Schema<'_>) -> Result<i64, SerdeError> {
        match self.current {
            Some(_) => parse_primitive::<i64>(&self.next_text()?, "long"),
            None => parse_primitive::<i64>(self.single_value()?, "long"),
        }
    }

    fn read_float(&mut self, _schema: &Schema<'_>) -> Result<f32, SerdeError> {
        match self.current {
            Some(_) => parse_primitive::<f32>(&self.next_text()?, "float"),
            None => parse_primitive::<f32>(self.single_value()?, "float"),
        }
    }

    fn read_double(&mut self, _schema: &Schema<'_>) -> Result<f64, SerdeError> {
        match self.current {
            Some(_) => parse_primitive::<f64>(&self.next_text()?, "double"),
            None => parse_primitive::<f64>(self.single_value()?, "double"),
        }
    }

    fn read_big_integer(&mut self, _schema: &Schema<'_>) -> Result<BigInteger, SerdeError> {
        use std::str::FromStr;
        let v = match self.current {
            Some(_) => self.next_text()?,
            None => Cow::Borrowed(self.single_value()?.trim()),
        };
        BigInteger::from_str(&v)
            .map_err(|_| SerdeError::invalid_input(format!("invalid big integer: {v}")))
    }

    fn read_big_decimal(&mut self, _schema: &Schema<'_>) -> Result<BigDecimal, SerdeError> {
        use std::str::FromStr;
        let v = match self.current {
            Some(_) => self.next_text()?,
            None => Cow::Borrowed(self.single_value()?.trim()),
        };
        BigDecimal::from_str(&v)
            .map_err(|_| SerdeError::invalid_input(format!("invalid big decimal: {v}")))
    }

    fn read_string(&mut self, schema: &Schema<'_>) -> Result<String, SerdeError> {
        let raw = match self.current {
            Some(_) => self.next_text()?,
            None => Cow::Borrowed(self.single_value()?.trim()),
        };
        let media_typed = schema.media_type().is_some() || self.member.media_type().is_some();
        if media_typed {
            let decoded = aws_smithy_types::base64::decode(&raw)
                .map_err(|err| SerdeError::invalid_input(format!("invalid base64: {err}")))?;
            String::from_utf8(decoded)
                .map_err(|_| SerdeError::invalid_input("base64-decoded header was not valid UTF-8"))
        } else {
            Ok(raw.into_owned())
        }
    }

    fn read_blob(&mut self, _schema: &Schema<'_>) -> Result<Blob, SerdeError> {
        let v = match self.current {
            Some(_) => self.next_text()?,
            None => Cow::Borrowed(self.single_value()?.trim()),
        };
        let decoded = aws_smithy_types::base64::decode(&v)
            .map_err(|err| SerdeError::invalid_input(format!("invalid base64: {err}")))?;
        Ok(Blob::new(decoded))
    }

    fn read_timestamp(&mut self, schema: &Schema<'_>) -> Result<DateTime, SerdeError> {
        match self.current {
            Some(_) => match self.next_token()? {
                HeaderToken::Date(dt) => Ok(dt),
                HeaderToken::Text(s) => {
                    let format =
                        resolve_timestamp_format(schema, self.member, BindingLocation::Header);
                    DateTime::from_str(&s, format).map_err(|err| {
                        SerdeError::invalid_input(format!("invalid timestamp: {err}"))
                    })
                }
            },
            None => {
                let format = resolve_timestamp_format(schema, self.member, BindingLocation::Header);
                let dates = many_dates(&self.values, format)?;
                match dates.len() {
                    1 => Ok(dates[0]),
                    0 => Err(SerdeError::invalid_input(
                        "expected a timestamp header value",
                    )),
                    _ => Err(SerdeError::invalid_input(
                        "expected a single timestamp header value but found multiple",
                    )),
                }
            }
        }
    }

    fn read_document(&mut self, _schema: &Schema<'_>) -> Result<Document, SerdeError> {
        Err(SerdeError::unsupported(
            "documents cannot be bound to headers",
        ))
    }

    fn is_null(&self) -> bool {
        false
    }

    fn container_size(&self) -> Option<usize> {
        self.list_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_schema::traits::TimestampFormat;
    use aws_smithy_schema::{Schema, ShapeId, ShapeType};

    static STRING_HEADER_SCHEMA: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("example#String", "example", "String"),
        ShapeType::String,
        "scalar",
        0,
    )
    .with_http_header("x-scalar");

    static STRING_LIST_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("example#String", "example", "String"),
        ShapeType::String,
        "member",
        0,
    );

    static STRING_LIST_HEADER_SCHEMA: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("example#StringList", "example", "StringList"),
        ShapeType::List,
        "values",
        0,
    )
    .with_list_member(&STRING_LIST_MEMBER_SCHEMA)
    .with_http_header("x-values");

    static TIMESTAMP_HEADER_SCHEMA: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("example#Timestamp", "example", "Timestamp"),
        ShapeType::Timestamp,
        "time",
        0,
    )
    .with_http_header("x-time");

    static MEDIA_TYPE_STRING_HEADER_SCHEMA: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("example#String", "example", "String"),
        ShapeType::String,
        "encoded",
        0,
    )
    .with_http_header("x-encoded")
    .with_media_type("text/plain");

    static BLOB_HEADER_SCHEMA: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("example#Blob", "example", "Blob"),
        ShapeType::Blob,
        "data",
        0,
    )
    .with_http_header("x-data");

    static TIMESTAMP_LIST_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("example#Timestamp", "example", "Timestamp"),
        ShapeType::Timestamp,
        "member",
        0,
    );

    static TIMESTAMP_LIST_HEADER_SCHEMA: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("example#TimestampList", "example", "TimestampList"),
        ShapeType::List,
        "times",
        0,
    )
    .with_list_member(&TIMESTAMP_LIST_MEMBER_SCHEMA)
    .with_http_header("x-times");

    static EPOCH_SECONDS_TIMESTAMP_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("example#Timestamp", "example", "Timestamp"),
        ShapeType::Timestamp,
        "member",
        0,
    )
    .with_timestamp_format(TimestampFormat::EpochSeconds);

    static EPOCH_SECONDS_TIMESTAMP_LIST_HEADER_SCHEMA: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("example#TimestampList", "example", "TimestampList"),
        ShapeType::List,
        "times",
        0,
    )
    .with_list_member(&EPOCH_SECONDS_TIMESTAMP_MEMBER_SCHEMA)
    .with_http_header("x-times");

    static DATE_TIME_TIMESTAMP_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("example#Timestamp", "example", "Timestamp"),
        ShapeType::Timestamp,
        "member",
        0,
    )
    .with_timestamp_format(TimestampFormat::DateTime);

    static DATE_TIME_TIMESTAMP_LIST_HEADER_SCHEMA: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("example#TimestampList", "example", "TimestampList"),
        ShapeType::List,
        "times",
        0,
    )
    .with_list_member(&DATE_TIME_TIMESTAMP_MEMBER_SCHEMA)
    .with_http_header("x-times");

    static STRING_MAP_KEY_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("example#String", "example", "String"),
        ShapeType::String,
        "key",
        0,
    );

    static STRING_MAP_VALUE_MEMBER_SCHEMA: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("example#String", "example", "String"),
        ShapeType::String,
        "value",
        1,
    );

    static PREFIX_HEADERS_MAP_SCHEMA: Schema<'static> = Schema::new_member(
        ShapeId::from_parts("example#StringMap", "example", "StringMap"),
        ShapeType::Map,
        "metadata",
        0,
    )
    .with_map_members(
        &STRING_MAP_KEY_MEMBER_SCHEMA,
        &STRING_MAP_VALUE_MEMBER_SCHEMA,
    )
    .with_http_prefix_headers("x-meta-");

    #[test]
    fn scalar_string_uses_dedicated_header_deserializer() {
        let mut deser = ScalarHeaderValueDeserializer::new(&STRING_HEADER_SCHEMA, " trace-123 ");

        let value = deser.read_string(&STRING_HEADER_SCHEMA).unwrap();

        assert_eq!(value, "trace-123");
    }

    #[test]
    fn scalar_timestamp_header_defaults_to_http_date() {
        let mut deser = ScalarHeaderValueDeserializer::new(
            &TIMESTAMP_HEADER_SCHEMA,
            "Tue, 14 Nov 2023 22:13:20 GMT",
        );

        let value = deser.read_timestamp(&TIMESTAMP_HEADER_SCHEMA).unwrap();

        assert_eq!(value.secs(), 1_700_000_000);
    }

    #[test]
    fn media_type_string_header_is_base64_decoded() {
        let mut deser =
            ScalarHeaderValueDeserializer::new(&MEDIA_TYPE_STRING_HEADER_SCHEMA, "c21pdGh5LXJz");

        let value = deser.read_string(&MEDIA_TYPE_STRING_HEADER_SCHEMA).unwrap();

        assert_eq!(value, "smithy-rs");
    }

    #[test]
    fn blob_header_is_base64_decoded() {
        let mut deser = ScalarHeaderValueDeserializer::new(&BLOB_HEADER_SCHEMA, "c21pdGh5LXJz");

        let value = deser.read_blob(&BLOB_HEADER_SCHEMA).unwrap();

        assert_eq!(value.as_ref(), b"smithy-rs");
    }

    #[test]
    fn string_list_splits_multiple_header_lines_and_quoted_commas() {
        let mut deser = HeaderValuesDeserializer::new(
            &STRING_LIST_HEADER_SCHEMA,
            vec![
                "tenant-a, tenant-b",
                "tenant-c",
                "\"tenant,d\"",
                "\"tenant\\\"e\"",
            ],
        );
        let mut out = Vec::new();

        deser
            .read_list(&STRING_LIST_HEADER_SCHEMA, &mut |element| {
                out.push(element.read_string(&STRING_LIST_MEMBER_SCHEMA)?);
                Ok(())
            })
            .unwrap();

        assert_eq!(
            out,
            ["tenant-a", "tenant-b", "tenant-c", "tenant,d", "tenant\"e"]
        );
    }

    #[test]
    fn timestamp_list_preserves_http_date_commas() {
        let mut deser = HeaderValuesDeserializer::one(
            &TIMESTAMP_LIST_HEADER_SCHEMA,
            "Tue, 14 Nov 2023 22:13:20 GMT, Wed, 15 Nov 2023 22:13:20 GMT",
        );
        let mut out = Vec::new();

        deser
            .read_list(&TIMESTAMP_LIST_HEADER_SCHEMA, &mut |element| {
                out.push(
                    element
                        .read_timestamp(&TIMESTAMP_LIST_MEMBER_SCHEMA)?
                        .secs(),
                );
                Ok(())
            })
            .unwrap();

        assert_eq!(out, [1_700_000_000, 1_700_086_400]);
    }

    #[test]
    fn timestamp_list_respects_explicit_epoch_seconds_format() {
        let mut deser =
            HeaderValuesDeserializer::one(&EPOCH_SECONDS_TIMESTAMP_LIST_HEADER_SCHEMA, "1, 2");
        let mut out = Vec::new();

        deser
            .read_list(
                &EPOCH_SECONDS_TIMESTAMP_LIST_HEADER_SCHEMA,
                &mut |element| {
                    out.push(
                        element
                            .read_timestamp(&EPOCH_SECONDS_TIMESTAMP_MEMBER_SCHEMA)?
                            .secs(),
                    );
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(out, [1, 2]);
    }

    #[test]
    fn timestamp_list_respects_explicit_date_time_format() {
        let mut deser = HeaderValuesDeserializer::one(
            &DATE_TIME_TIMESTAMP_LIST_HEADER_SCHEMA,
            "2023-11-14T22:13:20Z, 2023-11-15T22:13:20Z",
        );
        let mut out = Vec::new();

        deser
            .read_list(&DATE_TIME_TIMESTAMP_LIST_HEADER_SCHEMA, &mut |element| {
                out.push(
                    element
                        .read_timestamp(&DATE_TIME_TIMESTAMP_MEMBER_SCHEMA)?
                        .secs(),
                );
                Ok(())
            })
            .unwrap();

        assert_eq!(out, [1_700_000_000, 1_700_086_400]);
    }

    #[test]
    fn prefix_headers_are_read_as_map_entries() {
        let mut deser = PrefixHeadersDeserializer::new(
            &PREFIX_HEADERS_MAP_SCHEMA,
            vec![("color".to_string(), "red"), ("size".to_string(), "large")],
        );
        let mut out = std::collections::HashMap::new();

        deser
            .read_map(&PREFIX_HEADERS_MAP_SCHEMA, &mut |key, value| {
                out.insert(key, value.read_string(&STRING_MAP_VALUE_MEMBER_SCHEMA)?);
                Ok(())
            })
            .unwrap();

        assert_eq!(out.get("color").map(String::as_str), Some("red"));
        assert_eq!(out.get("size").map(String::as_str), Some("large"));
    }

    #[test]
    fn list_consumer_can_ignore_elements() {
        let mut deser =
            HeaderValuesDeserializer::one(&STRING_LIST_HEADER_SCHEMA, "tenant-a, tenant-b");
        let mut calls = 0;

        deser
            .read_list(&STRING_LIST_HEADER_SCHEMA, &mut |_element| {
                calls += 1;
                Ok(())
            })
            .unwrap();

        assert_eq!(calls, 2);
    }
}
