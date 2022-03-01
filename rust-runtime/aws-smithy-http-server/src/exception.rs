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

impl From<crate::rejection::RequestRejection> for SmithyFrameworkExceptionType {
    fn from(err: crate::rejection::RequestRejection) -> Self {
        SmithyFrameworkExceptionType::Serialization(crate::Error::new(err))
    }
}

impl From<crate::rejection::ResponseRejection> for SmithyFrameworkExceptionType {
    fn from(err: crate::rejection::ResponseRejection) -> Self {
        SmithyFrameworkExceptionType::Serialization(crate::Error::new(err))
    }
}
