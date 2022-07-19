/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Protocol helpers.
use crate::rejection::RequestRejection;
use crate::request::RequestParts;
use paste::paste;

/// Supported protocols.
#[derive(Debug, Clone, Copy)]
pub enum Protocol {
    RestJson1,
    RestXml,
    AwsJson10,
    AwsJson11,
}

/// Implement the content-type header validation for a request.
macro_rules! impl_content_type_validation {
    ($name:literal, $type: literal, $subtype:literal, $rejection:path) => {
        paste! {
            #[doc = concat!("Validates that the request has the standard `", $type, "/", $subtype, "` content-type header.")]
            pub fn [<check_ $name _content_type>]<B>(req: &RequestParts<B>) -> Result<(), RequestRejection> {
                let mime = req
                    .headers()
                    .ok_or($rejection)?
                    .get(http::header::CONTENT_TYPE)
                    .ok_or($rejection)?
                    .to_str()
                    .map_err(|_| $rejection)?
                    .parse::<mime::Mime>()
                    .map_err(|_| RequestRejection::MimeParse)?;
                if mime.type_() == $type && mime.subtype() == $subtype {
                    Ok(())
                } else {
                    Err($rejection)
                }
            }
        }
    };
}

impl_content_type_validation!(
    "rest_json_1",
    "application",
    "json",
    RequestRejection::MissingRestJson1ContentType
);

impl_content_type_validation!(
    "rest_xml",
    "application",
    "xml",
    RequestRejection::MissingRestXmlContentType
);

impl_content_type_validation!(
    "aws_json_10",
    "application",
    "x-amz-json-1.0",
    RequestRejection::MissingAwsJson10ContentType
);

impl_content_type_validation!(
    "aws_json_11",
    "application",
    "x-amz-json-1.1",
    RequestRejection::MissingAwsJson11ContentType
);

#[cfg(test)]
mod tests {
    use super::*;
    use http::Request;

    fn req(content_type: &str) -> RequestParts<&str> {
        let request = Request::builder()
            .header("content-type", content_type)
            .body("")
            .unwrap();
        RequestParts::new(request)
    }

    /// This macro validates the rejection type since we cannot implement `PartialEq`
    /// for `RequestRejection` as it is based on the crate error type, which uses
    /// `crate::error::BoxError`.
    macro_rules! validate_rejection_type {
        ($result:expr, $rejection:path) => {
            match $result {
                Ok(()) => panic!("Content-type validation is expected to fail"),
                Err(e) => match e {
                    $rejection => {}
                    _ => panic!("Error {} should be {}", e.to_string(), stringify!($rejection)),
                },
            }
        };
    }

    #[test]
    fn validate_rest_json_1_content_type() {
        // Check valid content-type header.
        let request = req("application/json");
        assert!(check_rest_json_1_content_type(&request).is_ok());

        // Check invalid content-type header.
        let invalid = vec![
            req("application/ajson"),
            req("application/json1"),
            req("applicatio/json"),
            req("application/xml"),
            req("text/xml"),
            req("application/x-amz-json-1.0"),
            req("application/x-amz-json-1.1"),
            RequestParts::new(Request::builder().body("").unwrap()),
        ];
        for request in &invalid {
            validate_rejection_type!(
                check_rest_json_1_content_type(request),
                RequestRejection::MissingRestJson1ContentType
            );
        }

        // Check request with not parsable content-type header.
        validate_rejection_type!(check_rest_json_1_content_type(&req("123")), RequestRejection::MimeParse);
    }

    #[test]
    fn validate_rest_xml_content_type() {
        // Check valid content-type header.
        let request = req("application/xml");
        assert!(check_rest_xml_content_type(&request).is_ok());

        // Check invalid content-type header.
        let invalid = vec![
            req("application/axml"),
            req("application/xml1"),
            req("applicatio/xml"),
            req("text/xml"),
            req("application/x-amz-json-1.0"),
            req("application/x-amz-json-1.1"),
            RequestParts::new(Request::builder().body("").unwrap()),
        ];
        for request in &invalid {
            validate_rejection_type!(
                check_rest_xml_content_type(request),
                RequestRejection::MissingRestXmlContentType
            );
        }

        // Check request with not parsable content-type header.
        validate_rejection_type!(check_rest_xml_content_type(&req("123")), RequestRejection::MimeParse);
    }

    #[test]
    fn validate_aws_json_10_content_type() {
        // Check valid content-type header.
        let request = req("application/x-amz-json-1.0");
        assert!(check_aws_json_10_content_type(&request).is_ok());

        // Check invalid content-type header.
        let invalid = vec![
            req("application/x-amz-json-1."),
            req("application/-amz-json-1.0"),
            req("application/xml"),
            req("application/json"),
            req("applicatio/x-amz-json-1.0"),
            req("text/xml"),
            req("application/x-amz-json-1.1"),
            RequestParts::new(Request::builder().body("").unwrap()),
        ];
        for request in &invalid {
            validate_rejection_type!(
                check_aws_json_10_content_type(request),
                RequestRejection::MissingAwsJson10ContentType
            );
        }

        // Check request with not parsable content-type header.
        validate_rejection_type!(check_aws_json_10_content_type(&req("123")), RequestRejection::MimeParse);
    }

    #[test]
    fn validate_aws_json_11_content_type() {
        // Check valid content-type header.
        let request = req("application/x-amz-json-1.1");
        assert!(check_aws_json_11_content_type(&request).is_ok());

        // Check invalid content-type header.
        let invalid = vec![
            req("application/x-amz-json-1."),
            req("application/-amz-json-1.1"),
            req("application/xml"),
            req("application/json"),
            req("applicatio/x-amz-json-1.1"),
            req("text/xml"),
            req("application/x-amz-json-1.0"),
            RequestParts::new(Request::builder().body("").unwrap()),
        ];
        for request in &invalid {
            validate_rejection_type!(
                check_aws_json_11_content_type(request),
                RequestRejection::MissingAwsJson11ContentType
            );
        }

        // Check request with not parsable content-type header.
        validate_rejection_type!(check_aws_json_11_content_type(&req("123")), RequestRejection::MimeParse);
    }
}
