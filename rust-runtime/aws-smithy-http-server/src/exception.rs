use crate::protocols::Protocol;

#[derive(Debug)]
pub enum SmithyFrameworkExceptionType {
    // UnknownOperation,
    Serialization(crate::Error),
    // UnsupportedMediaType,
    // NotAcceptable,
}

impl SmithyFrameworkExceptionType {
    fn name(&self) -> &'static str {
        match self {
            SmithyFrameworkExceptionType::Serialization(_) => "SerializationException",
        }
    }
}

// In `FromRequest`, we call the deserializer, which might return an error `FromRequest`. We
// convert that into `SmithyFrameworkExceptionType`, we then create `SmithyFrameworkException`, and
// we return that.
#[derive(Debug)]
pub struct SmithyFrameworkException {
    pub protocol: Protocol,
    pub exception_type: SmithyFrameworkExceptionType,
}

// TODO Better implement Display and get ToString for free.
impl ToString for SmithyFrameworkException {
    fn to_string(&self) -> String {
        // TODO
        todo!()
    }
}

// pub struct RestJson1SmithyFrameworkExceptionType(SmithyFrameworkExceptionType);

// impl From<SmithyFrameworkExceptionType> for RestJson1SmithyFrameworkExceptionType {
//     fn from(inner: SmithyFrameworkExceptionType) -> Self {
//         Self(inner)
//     }
// }

impl axum_core::response::IntoResponse for SmithyFrameworkException {
    fn into_response(self) -> axum_core::response::Response {
        let status_code = match self.exception_type {
            SmithyFrameworkExceptionType::Serialization(_) => http::StatusCode::BAD_REQUEST,
        };

        let headers = match self.protocol {
            Protocol::RestJson1 => [
                ("Content-Type", "application/json"),
                ("X-Amzn-Errortype", self.exception_type.name()),
            ],
        };

        let body = crate::body::to_boxed(match self.protocol {
            Protocol::RestJson1 => "{}",
        });

        let mut builder = http::Response::builder();
        builder = builder.status(status_code);
        for (header_name, header_value) in headers {
            builder = builder.header(header_name, header_value);
        }

        // TODO What extension type should we use here?
        // TODO `ResponseExtensions` should probably be renamed, as it's something
        // operation-specific. `SmithyFrameworkException` might not have reached an operation
        // (think `UnknownOperationException`).
        builder = builder.extension(crate::ResponseExtensions::new("TODO", "TODO"));

        builder.body(body).expect("invalid HTTP response for `SmithyFrameworkException`; please file a bug report under https://github.com/awslabs/smithy-rs/issues")
    }
}

// rejections
//

// Deserialization functions return this as error.
// TODO Document precisely when all of these are created.
#[derive(Debug)]
pub enum FromRequest {
    /// Used when percent decoding query string.
    InvalidUtf8(crate::Error),
    JsonDeserialize(crate::Error),
    XmlDeserialize(crate::Error),
    BodyAlreadyExtracted,
    HeadersAlreadyExtracted,
    /// Used when parsing HTTP headers that are bound to input members (httpHeader,
    /// httpPrefixHeaders).
    HeaderParse(crate::Error),

    DateTimeParse(crate::Error),

    /// TODO This one should replace the below 3 when we refactor parsing code to use aws_smithy_types
    /// parse.
    PrimitiveParse(crate::Error),

    /// Maybe these 3 is too much detail. Maybe not.
    IntParse(crate::Error),
    FloatParse(crate::Error),
    BoolParse(crate::Error),

    /// When the nom parser's input does not match the URI pattern.
    UriPatternMismatch(crate::Error),
    Build(crate::Error),
    // TODO when hyper.to_bytes() fails
    HttpBody(crate::Error),
    // TODO We need to handwrite `ContentTypeRejection`.
    ContentType(crate::Error),
}

impl std::fmt::Display for FromRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // TODO Fill out.
        // TODO Consider deriving it with `enum_display_derive` or `thiserror`.
        write!(f, "{}", self)
    }
}

impl std::error::Error for FromRequest {}

impl From<FromRequest> for SmithyFrameworkExceptionType {
    fn from(err: FromRequest) -> Self {
        SmithyFrameworkExceptionType::Serialization(crate::Error::new(err))
    }
}

// TODO These could be generated with a macro

impl From<aws_smithy_json::deserialize::Error> for FromRequest {
    fn from(err: aws_smithy_json::deserialize::Error) -> Self {
        Self::JsonDeserialize(crate::Error::new(err))
    }
}

impl From<aws_smithy_xml::decode::XmlError> for FromRequest {
    fn from(err: aws_smithy_xml::decode::XmlError) -> Self {
        Self::XmlDeserialize(crate::Error::new(err))
    }
}

impl From<aws_smithy_http::operation::BuildError> for FromRequest {
    fn from(err: aws_smithy_http::operation::BuildError) -> Self {
        Self::Build(crate::Error::new(err))
    }
}

impl From<aws_smithy_http::header::ParseError> for FromRequest {
    fn from(err: aws_smithy_http::header::ParseError) -> Self {
        Self::HeaderParse(crate::Error::new(err))
    }
}

impl From<aws_smithy_types::date_time::DateTimeParseError> for FromRequest {
    fn from(err: aws_smithy_types::date_time::DateTimeParseError) -> Self {
        Self::DateTimeParse(crate::Error::new(err))
    }
}

impl From<aws_smithy_types::primitive::PrimitiveParseError> for FromRequest {
    fn from(err: aws_smithy_types::primitive::PrimitiveParseError) -> Self {
        Self::PrimitiveParse(crate::Error::new(err))
    }
}

impl From<std::str::ParseBoolError> for FromRequest {
    fn from(err: std::str::ParseBoolError) -> Self {
        Self::BoolParse(crate::Error::new(err))
    }
}

impl From<std::num::ParseFloatError> for FromRequest {
    fn from(err: std::num::ParseFloatError) -> Self {
        Self::FloatParse(crate::Error::new(err))
    }
}

impl From<std::num::ParseIntError> for FromRequest {
    fn from(err: std::num::ParseIntError) -> Self {
        Self::IntParse(crate::Error::new(err))
    }
}

impl From<nom::Err<nom::error::Error<&str>>> for FromRequest {
    // TODO Can't we make the parser return `Error<String>`?
    fn from(err: nom::Err<nom::error::Error<&str>>) -> Self {
        Self::UriPatternMismatch(crate::Error::new(err.to_owned()))
    }
}

// TODO I'm not sure we need this one. Because serde_urlencoded would have already failed if it's
// not UTF-8.
// i.e. do we really need `percent_encoding::percent_decode_str(value)?
impl From<std::str::Utf8Error> for FromRequest {
    fn from(err: std::str::Utf8Error) -> Self {
        Self::InvalidUtf8(crate::Error::new(err))
    }
}

impl From<serde_urlencoded::de::Error> for FromRequest {
    fn from(err: serde_urlencoded::de::Error) -> Self {
        Self::InvalidUtf8(crate::Error::new(err))
    }
}

// impl From<http::Error> for FromRequest {
//     fn from(err: http::Error) -> Self {
//         Self::HttpBody(crate::Error::new(err))
//     }
// }

/// `[crate::body::Body]` is `[hyper::Body]`, whose associated `Error` type is `[hyper::Error]`. We
/// need this converter for when we convert the body into bytes in the framework, since protocol
/// tests use `[crate::body::Body]` as their body type when constructing requests.
impl From<hyper::Error> for FromRequest {
    fn from(err: hyper::Error) -> Self {
        Self::HttpBody(crate::Error::new(err))
    }
}

impl From<crate::rejection::ContentTypeRejection> for FromRequest {
    fn from(err: crate::rejection::ContentTypeRejection) -> Self {
        Self::ContentType(crate::Error::new(err))
    }
}
