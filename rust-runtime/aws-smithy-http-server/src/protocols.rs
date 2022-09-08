/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Protocol helpers.
use crate::rejection::MissingContentTypeReason;
use crate::request::RequestParts;

/// [AWS REST JSON 1.0 Protocol](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-restjson1-protocol.html).
pub struct AwsRestJson1;

/// [AWS REST XML Protocol](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-restxml-protocol.html).
pub struct AwsRestXml;

/// [AWS JSON 1.0 Protocol](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-json-1_0-protocol.html).
pub struct AwsJson10;

/// [AWS JSON 1.1 Protocol](https://awslabs.github.io/smithy/2.0/aws/protocols/aws-json-1_1-protocol.html).
pub struct AwsJson11;

/// Supported protocols.
#[derive(Debug, Clone, Copy)]
pub enum Protocol {
    RestJson1,
    RestXml,
    AwsJson10,
    AwsJson11,
}

pub fn check_content_type<B>(
    req: &RequestParts<B>,
    expected_mime: &'static mime::Mime,
) -> Result<(), MissingContentTypeReason> {
    let found_mime = req
        .headers()
        .ok_or(MissingContentTypeReason::HeadersTakenByAnotherExtractor)?
        .get(http::header::CONTENT_TYPE)
        .ok_or(MissingContentTypeReason::NoContentTypeHeader)?
        .to_str()
        .map_err(MissingContentTypeReason::ToStrError)?
        .parse::<mime::Mime>()
        .map_err(MissingContentTypeReason::MimeParseError)?;
    if &found_mime == expected_mime {
        Ok(())
    } else {
        Err(MissingContentTypeReason::UnexpectedMimeType {
            expected_mime,
            found_mime,
        })
    }
}

pub fn accept_header_classifier<B>(req: &RequestParts<B>, content_type: &'static str) -> bool {
    // Allow no ACCEPT header
    if req.headers().is_none() {
        return true;
    }
    let headers = req.headers().unwrap();
    if !headers.contains_key(http::header::ACCEPT) {
        return true;
    }
    // Must be of the form: type/subtype
    let content_type = content_type
        .parse::<mime::Mime>()
        .expect("BUG: MIME parsing failed, content_type is not valid");
    headers
        .get_all(http::header::ACCEPT)
        .into_iter()
        .flat_map(|header| {
            header
                .to_str()
                .ok()
                .into_iter()
                /*
                 * turn a header value of: "type0/subtype0, type1/subtype1, ..."
                 * into: ["type0/subtype0", "type1/subtype1", ...]
                 * and remove the optional "; q=x" parameters
                 * NOTE: the unwrap() is safe, because it takes the first element (if there's nothing to split, returns the string)
                 */
                .flat_map(|s| s.split(',').map(|typ| typ.split(';').next().unwrap().trim()))
        })
        .filter_map(|h| h.parse::<mime::Mime>().ok())
        .any(|mim| {
            let typ = content_type.type_();
            let subtype = content_type.subtype();
            // Accept: */*, type/*, type/subtype
            match (mim.type_(), mim.subtype()) {
                (t, s) if t == typ && s == subtype => true,
                (t, mime::STAR) if t == typ => true,
                (mime::STAR, mime::STAR) => true,
                _ => false,
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Request;

    fn req_content_type(content_type: &str) -> RequestParts<&str> {
        let request = Request::builder()
            .header("content-type", content_type)
            .body("")
            .unwrap();
        RequestParts::new(request)
    }

    fn req_accept(content_type: &str) -> RequestParts<&str> {
        let request = Request::builder().header("accept", content_type).body("").unwrap();
        RequestParts::new(request)
    }

    static EXPECTED_MIME_APPLICATION_JSON: once_cell::sync::Lazy<mime::Mime> =
        once_cell::sync::Lazy::new(|| "application/json".parse::<mime::Mime>().unwrap());

    #[test]
    fn check_valid_content_type() {
        let valid_request = req_content_type("application/json");
        assert!(check_content_type(&valid_request, &EXPECTED_MIME_APPLICATION_JSON).is_ok());
    }

    #[test]
    fn check_invalid_content_type() {
        let invalid = vec!["application/ajson", "text/xml"];
        for invalid_mime in invalid {
            let request = req_content_type(invalid_mime);
            let result = check_content_type(&request, &EXPECTED_MIME_APPLICATION_JSON);

            // Validates the rejection type since we cannot implement `PartialEq`
            // for `MissingContentTypeReason`.
            match result {
                Ok(()) => panic!("Content-type validation is expected to fail"),
                Err(e) => match e {
                    MissingContentTypeReason::UnexpectedMimeType {
                        expected_mime,
                        found_mime,
                    } => {
                        assert_eq!(expected_mime, &"application/json".parse::<mime::Mime>().unwrap());
                        assert_eq!(found_mime, invalid_mime);
                    }
                    _ => panic!("Unexpected `MissingContentTypeReason`: {}", e.to_string()),
                },
            }
        }
    }

    #[test]
    fn check_missing_content_type() {
        let request = RequestParts::new(Request::builder().body("").unwrap());
        let result = check_content_type(&request, &EXPECTED_MIME_APPLICATION_JSON);
        assert!(matches!(
            result.unwrap_err(),
            MissingContentTypeReason::NoContentTypeHeader
        ));
    }

    #[test]
    fn check_not_parsable_content_type() {
        let request = req_content_type("123");
        let result = check_content_type(&request, &EXPECTED_MIME_APPLICATION_JSON);
        assert!(matches!(
            result.unwrap_err(),
            MissingContentTypeReason::MimeParseError(_)
        ));
    }

    #[test]
    fn check_non_ascii_visible_characters_content_type() {
        let request = req_content_type("application/💩");
        let result = check_content_type(&request, &EXPECTED_MIME_APPLICATION_JSON);
        assert!(matches!(result.unwrap_err(), MissingContentTypeReason::ToStrError(_)));
    }

    #[test]
    fn valid_accept_header_classifier_multiple_values() {
        let valid_request = req_accept("text/strings, application/json, invalid");
        assert!(accept_header_classifier(&valid_request, "application/json"));
    }

    #[test]
    fn invalid_accept_header_classifier() {
        let invalid_request = req_accept("text/invalid, invalid, invalid/invalid");
        assert!(!accept_header_classifier(&invalid_request, "application/json"));
    }

    #[test]
    fn valid_accept_header_classifier_star() {
        let valid_request = req_accept("application/*");
        assert!(accept_header_classifier(&valid_request, "application/json"));
    }

    #[test]
    fn valid_accept_header_classifier_star_star() {
        let valid_request = req_accept("*/*");
        assert!(accept_header_classifier(&valid_request, "application/json"));
    }

    #[test]
    fn valid_empty_accept_header_classifier() {
        let valid_request = Request::builder().body("").unwrap();
        let valid_request = RequestParts::new(valid_request);
        assert!(accept_header_classifier(&valid_request, "application/json"));
    }

    #[test]
    fn valid_accept_header_classifier_with_params() {
        let valid_request = req_accept("application/json; q=30, */*");
        assert!(accept_header_classifier(&valid_request, "application/json"));
    }

    #[test]
    fn valid_accept_header_classifier() {
        let valid_request = req_accept("application/json");
        assert!(accept_header_classifier(&valid_request, "application/json"));
    }
}
