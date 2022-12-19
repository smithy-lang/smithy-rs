/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use http::{HeaderMap, HeaderValue};

const EXTENDED_REQUEST_ID: &str = "s3_extended_request_id";

/// S3-specific service error additions.
pub trait ErrorExt {
    /// Returns the S3 Extended Request ID necessary when contacting AWS Support.
    /// Read more at <https://aws.amazon.com/premiumsupport/knowledge-center/s3-request-id-values/>.
    fn extended_request_id(&self) -> Option<&str>;
}

impl ErrorExt for aws_smithy_types::Error {
    fn extended_request_id(&self) -> Option<&str> {
        self.extra(EXTENDED_REQUEST_ID)
    }
}

/// Parses the S3 Extended Request ID out of S3 error response headers.
pub fn apply_extended_error(
    builder: aws_smithy_types::error::Builder,
    headers: &HeaderMap<HeaderValue>,
) -> aws_smithy_types::error::Builder {
    let host_id = headers
        .get("x-amz-id-2")
        .and_then(|header_value| header_value.to_str().ok());
    if let Some(host_id) = host_id {
        builder.custom(EXTENDED_REQUEST_ID, host_id)
    } else {
        builder
    }
}

#[cfg(test)]
mod test {
    use crate::s3_errors::{apply_extended_error, ErrorExt};

    #[test]
    fn add_error_fields() {
        let resp = http::Response::builder()
            .header(
                "x-amz-id-2",
                "eftixk72aD6Ap51TnqcoF8eFidJG9Z/2mkiDFu8yU9AS1ed4OpIszj7UDNEHGran",
            )
            .status(400)
            .body("")
            .unwrap();
        let mut builder = aws_smithy_types::Error::builder().message("123");
        builder = apply_extended_error(builder, resp.headers());
        assert_eq!(
            builder
                .build()
                .extended_request_id()
                .expect("extended request id should be set"),
            "eftixk72aD6Ap51TnqcoF8eFidJG9Z/2mkiDFu8yU9AS1ed4OpIszj7UDNEHGran"
        );
    }

    #[test]
    fn handle_missing_header() {
        let resp = http::Response::builder().status(400).body("").unwrap();
        let mut builder = aws_smithy_types::Error::builder().message("123");
        builder = apply_extended_error(builder, resp.headers());
        assert_eq!(builder.build().extended_request_id(), None);
    }
}
