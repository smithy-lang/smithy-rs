/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http::http::HttpHeaders;
use aws_smithy_http::operation;
use aws_smithy_http::result::SdkError;
use aws_smithy_types::error::{Builder as GenericErrorBuilder, Error as GenericError};
use http::{HeaderMap, HeaderValue};

/// Constant for the [`aws_smithy_types::error::Error`] extra field that contains the request ID
const AWS_REQUEST_ID: &str = "aws_request_id";

/// Implementers add a function to return an AWS request ID
pub trait RequestId {
    /// Returns the request ID if it's available.
    fn request_id(&self) -> Option<&str>;
}

impl<E, R> RequestId for SdkError<E, R>
where
    R: HttpHeaders,
{
    fn request_id(&self) -> Option<&str> {
        match self {
            Self::ResponseError(err) => extract_request_id(err.raw().http_headers()),
            Self::ServiceError(err) => extract_request_id(err.raw().http_headers()),
            _ => None,
        }
    }
}

impl RequestId for GenericError {
    fn request_id(&self) -> Option<&str> {
        self.extra(AWS_REQUEST_ID)
    }
}

impl RequestId for operation::Response {
    fn request_id(&self) -> Option<&str> {
        extract_request_id(self.http().headers())
    }
}

impl<O, E> RequestId for Result<O, E>
where
    O: RequestId,
    E: RequestId,
{
    fn request_id(&self) -> Option<&str> {
        match self {
            Ok(ok) => ok.request_id(),
            Err(err) => err.request_id(),
        }
    }
}

/// Applies a request ID to a generic error builder
#[doc(hidden)]
pub fn apply_request_id(
    builder: GenericErrorBuilder,
    headers: &HeaderMap<HeaderValue>,
) -> GenericErrorBuilder {
    if let Some(request_id) = extract_request_id(headers) {
        builder.custom(AWS_REQUEST_ID, request_id)
    } else {
        builder
    }
}

/// Extracts a request ID from HTTP response headers
fn extract_request_id(headers: &HeaderMap<HeaderValue>) -> Option<&str> {
    headers
        .get("x-amzn-requestid")
        .or_else(|| headers.get("x-amz-request-id"))
        .map(|value| std::str::from_utf8(value.as_bytes()).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_http::body::SdkBody;
    use http::Response;

    #[test]
    fn test_request_id_sdk_error() {
        let without_request_id =
            || operation::Response::new(Response::builder().body(SdkBody::empty()).unwrap());
        let with_request_id = || {
            operation::Response::new(
                Response::builder()
                    .header(
                        "x-amzn-requestid",
                        HeaderValue::from_static("some-request-id"),
                    )
                    .body(SdkBody::empty())
                    .unwrap(),
            )
        };
        assert_eq!(
            None,
            SdkError::<(), _>::response_error("test", without_request_id()).request_id()
        );
        assert_eq!(
            Some("some-request-id"),
            SdkError::<(), _>::response_error("test", with_request_id()).request_id()
        );
        assert_eq!(
            None,
            SdkError::service_error((), without_request_id()).request_id()
        );
        assert_eq!(
            Some("some-request-id"),
            SdkError::service_error((), with_request_id()).request_id()
        );
    }

    #[test]
    fn test_extract_request_id() {
        let mut headers = HeaderMap::new();
        assert_eq!(None, extract_request_id(&headers));

        headers.append(
            "x-amzn-requestid",
            HeaderValue::from_static("some-request-id"),
        );
        assert_eq!(Some("some-request-id"), extract_request_id(&headers));

        headers.append(
            "x-amz-request-id",
            HeaderValue::from_static("other-request-id"),
        );
        assert_eq!(Some("some-request-id"), extract_request_id(&headers));

        headers.remove("x-amzn-requestid");
        assert_eq!(Some("other-request-id"), extract_request_id(&headers));
    }

    #[test]
    fn test_apply_request_id() {
        let mut headers = HeaderMap::new();
        assert_eq!(
            GenericError::builder().build(),
            apply_request_id(GenericError::builder(), &headers).build(),
        );

        headers.append(
            "x-amzn-requestid",
            HeaderValue::from_static("some-request-id"),
        );
        assert_eq!(
            GenericError::builder()
                .custom(AWS_REQUEST_ID, "some-request-id")
                .build(),
            apply_request_id(GenericError::builder(), &headers).build(),
        );
    }

    #[test]
    fn test_generic_error_request_id_impl() {
        let err = GenericError::builder()
            .custom(AWS_REQUEST_ID, "some-request-id")
            .build();
        assert_eq!(Some("some-request-id"), err.request_id());
    }
}
