/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

pub mod aws_json;
pub mod aws_json_10;
pub mod aws_json_11;
pub mod rest;
pub mod rest_json_1;
pub mod rest_xml;

use crate::rejection::MissingContentTypeReason;
use http::HeaderMap;

#[cfg(test)]
pub mod test_helpers {
    use http::{HeaderMap, Method, Request};

    /// Helper function to build a `Request`. Used in other test modules.
    pub fn req(method: &Method, uri: &str, headers: Option<HeaderMap>) -> Request<()> {
        let mut r = Request::builder().method(method).uri(uri).body(()).unwrap();
        if let Some(headers) = headers {
            *r.headers_mut() = headers
        }
        r
    }

    // Returns a `Response`'s body as a `String`, without consuming the response.
    pub async fn get_body_as_string<B>(body: B) -> String
    where
        B: http_body::Body + std::marker::Unpin,
        B::Error: std::fmt::Debug,
    {
        let body_bytes = hyper::body::to_bytes(body).await.unwrap();
        String::from(std::str::from_utf8(&body_bytes).unwrap())
    }
}

/// When there are no modeled inputs,
/// a request body is empty and the content-type request header must not be set
#[allow(clippy::result_large_err)]
pub fn content_type_header_empty_body_no_modeled_input(headers: &HeaderMap) -> Result<(), MissingContentTypeReason> {
    if headers.contains_key(http::header::CONTENT_TYPE) {
        let found_mime = parse_content_type(headers)?;
        Err(MissingContentTypeReason::UnexpectedMimeType {
            expected_mime: None,
            found_mime: Some(found_mime),
        })
    } else {
        Ok(())
    }
}

#[allow(clippy::result_large_err)]
fn parse_content_type(headers: &HeaderMap) -> Result<mime::Mime, MissingContentTypeReason> {
    headers
        .get(http::header::CONTENT_TYPE)
        .unwrap() // The header is present, `unwrap` will not panic.
        .to_str()
        .map_err(MissingContentTypeReason::ToStrError)?
        .parse::<mime::Mime>()
        .map_err(MissingContentTypeReason::MimeParseError)
}

/// Checks that the `content-type` header is valid.
#[allow(clippy::result_large_err)]
pub fn content_type_header_classifier(
    headers: &HeaderMap,
    expected_content_type: Option<&'static str>,
) -> Result<(), MissingContentTypeReason> {
    if !headers.contains_key(http::header::CONTENT_TYPE) {
        return Ok(());
    }
    let found_mime = parse_content_type(headers)?;
    // There is a `content-type` header.
    // If there is an implied content type, they must match.
    if let Some(expected_content_type) = expected_content_type {
        let expected_mime = expected_content_type
            .parse::<mime::Mime>()
            // `expected_content_type` comes from the codegen.
            .expect("BUG: MIME parsing failed, `expected_content_type` is not valid. Please file a bug report under https://github.com/awslabs/smithy-rs/issues");
        if expected_content_type != found_mime {
            return Err(MissingContentTypeReason::UnexpectedMimeType {
                expected_mime: Some(expected_mime),
                found_mime: Some(found_mime),
            });
        }
    } else {
        // `content-type` header and no modeled input (mismatch).
        return Err(MissingContentTypeReason::UnexpectedMimeType {
            expected_mime: None,
            found_mime: Some(found_mime),
        });
    }
    Ok(())
}

pub fn accept_header_classifier(headers: &HeaderMap, content_type: &mime::Mime) -> bool {
    if !headers.contains_key(http::header::ACCEPT) {
        return true;
    }
    headers
        .get_all(http::header::ACCEPT)
        .into_iter()
        .flat_map(|header| {
            header
                .to_str()
                .ok()
                .into_iter()
                /*
                 * Turn a header value of: "type0/subtype0, type1/subtype1, ..."
                 * into: ["type0/subtype0", "type1/subtype1", ...]
                 * and remove the optional "; q=x" parameters
                 * NOTE: the `unwrap`() is safe, because it takes the first element (if there's nothing to split, returns the string)
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
    use http::header::{HeaderValue, ACCEPT, CONTENT_TYPE};

    fn req_content_type(content_type: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_str(content_type).unwrap());
        headers
    }

    fn req_accept(accept: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static(accept));
        headers
    }

    const EXPECTED_MIME_APPLICATION_JSON: Option<&'static str> = Some("application/json");

    #[test]
    fn check_content_type_header_empty_body_no_modeled_input() {
        assert!(content_type_header_empty_body_no_modeled_input(&HeaderMap::new()).is_ok());
    }

    #[test]
    fn check_invalid_content_type_header_empty_body_no_modeled_input() {
        let valid_request = req_content_type("application/json");
        let result = content_type_header_empty_body_no_modeled_input(&valid_request).unwrap_err();
        assert!(matches!(
            result,
            MissingContentTypeReason::UnexpectedMimeType {
                expected_mime: None,
                found_mime: Some(_)
            }
        ));
    }

    #[test]
    fn check_invalid_content_type() {
        let invalid = vec!["application/jason", "text/xml"];
        for invalid_mime in invalid {
            let request = req_content_type(invalid_mime);
            let result = content_type_header_classifier(&request, EXPECTED_MIME_APPLICATION_JSON);

            // Validates the rejection type since we cannot implement `PartialEq`
            // for `MissingContentTypeReason`.
            match result {
                Ok(()) => panic!("Content-type validation is expected to fail"),
                Err(e) => match e {
                    MissingContentTypeReason::UnexpectedMimeType {
                        expected_mime,
                        found_mime,
                    } => {
                        assert_eq!(
                            expected_mime.unwrap(),
                            "application/json".parse::<mime::Mime>().unwrap()
                        );
                        assert_eq!(found_mime, invalid_mime.parse::<mime::Mime>().ok());
                    }
                    _ => panic!("Unexpected `MissingContentTypeReason`: {}", e),
                },
            }
        }
    }

    #[test]
    fn check_missing_content_type_is_allowed() {
        let result = content_type_header_classifier(&HeaderMap::new(), EXPECTED_MIME_APPLICATION_JSON);
        assert!(result.is_ok());
    }

    #[test]
    fn check_not_parsable_content_type() {
        let request = req_content_type("123");
        let result = content_type_header_classifier(&request, EXPECTED_MIME_APPLICATION_JSON);
        assert!(matches!(
            result.unwrap_err(),
            MissingContentTypeReason::MimeParseError(_)
        ));
    }

    #[test]
    fn check_non_ascii_visible_characters_content_type() {
        let request = req_content_type("application/💩");
        let result = content_type_header_classifier(&request, EXPECTED_MIME_APPLICATION_JSON);
        assert!(matches!(result.unwrap_err(), MissingContentTypeReason::ToStrError(_)));
    }

    #[test]
    fn valid_accept_header_classifier_multiple_values() {
        let valid_request = req_accept("text/strings, application/json, invalid");
        assert!(accept_header_classifier(
            &valid_request,
            &"application/json".parse().unwrap()
        ));
    }

    #[test]
    fn invalid_accept_header_classifier() {
        let invalid_request = req_accept("text/invalid, invalid, invalid/invalid");
        assert!(!accept_header_classifier(
            &invalid_request,
            &"application/json".parse().unwrap()
        ));
    }

    #[test]
    fn valid_accept_header_classifier_star() {
        let valid_request = req_accept("application/*");
        assert!(accept_header_classifier(
            &valid_request,
            &"application/json".parse().unwrap()
        ));
    }

    #[test]
    fn valid_accept_header_classifier_star_star() {
        let valid_request = req_accept("*/*");
        assert!(accept_header_classifier(
            &valid_request,
            &"application/json".parse().unwrap()
        ));
    }

    #[test]
    fn valid_empty_accept_header_classifier() {
        assert!(accept_header_classifier(
            &HeaderMap::new(),
            &"application/json".parse().unwrap()
        ));
    }

    #[test]
    fn valid_accept_header_classifier_with_params() {
        let valid_request = req_accept("application/json; q=30, */*");
        assert!(accept_header_classifier(
            &valid_request,
            &"application/json".parse().unwrap()
        ));
    }

    #[test]
    fn valid_accept_header_classifier() {
        let valid_request = req_accept("application/json");
        assert!(accept_header_classifier(
            &valid_request,
            &"application/json".parse().unwrap()
        ));
    }
}
