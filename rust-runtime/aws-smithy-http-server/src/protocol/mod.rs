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
pub mod rpc_v2_cbor;

use crate::rejection::MissingContentTypeReason;
use aws_smithy_runtime_api::http::Headers as SmithyHeaders;
use http::header::CONTENT_TYPE;
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

#[allow(clippy::result_large_err)]
fn parse_mime(content_type: &str) -> Result<mime::Mime, MissingContentTypeReason> {
    content_type
        .parse::<mime::Mime>()
        .map_err(MissingContentTypeReason::MimeParseError)
}

/// Checks that the `content-type` header from a `SmithyHeaders` matches what we expect.
#[allow(clippy::result_large_err)]
pub fn content_type_header_classifier_smithy(
    headers: &SmithyHeaders,
    expected_content_type: Option<&'static str>,
) -> Result<(), MissingContentTypeReason> {
    let actual_content_type = headers.get(CONTENT_TYPE);
    content_type_header_classifier(actual_content_type, expected_content_type)
}

/// Checks that the `content-type` header matches what we expect.
#[allow(clippy::result_large_err)]
fn content_type_header_classifier(
    actual_content_type: Option<&str>,
    expected_content_type: Option<&'static str>,
) -> Result<(), MissingContentTypeReason> {
    fn parse_expected_mime(expected_content_type: &str) -> mime::Mime {
        let mime = expected_content_type
                .parse::<mime::Mime>()
                // `expected_content_type` comes from the codegen.
                .expect("BUG: MIME parsing failed, `expected_content_type` is not valid; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues");
        debug_assert_eq!(
            mime, expected_content_type,
            "BUG: expected `content-type` header value we own from codegen should coincide with its mime type; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues",
        );
        mime
    }

    match (actual_content_type, expected_content_type) {
        (None, None) => Ok(()),
        (None, Some(expected_content_type)) => {
            let expected_mime = parse_expected_mime(expected_content_type);
            Err(MissingContentTypeReason::UnexpectedMimeType {
                expected_mime: Some(expected_mime),
                found_mime: None,
            })
        }
        (Some(actual_content_type), None) => {
            let found_mime = parse_mime(actual_content_type)?;
            Err(MissingContentTypeReason::UnexpectedMimeType {
                expected_mime: None,
                found_mime: Some(found_mime),
            })
        }
        (Some(actual_content_type), Some(expected_content_type)) => {
            let expected_mime = parse_expected_mime(expected_content_type);
            let found_mime = parse_mime(actual_content_type)?;
            if expected_mime != found_mime.essence_str() {
                Err(MissingContentTypeReason::UnexpectedMimeType {
                    expected_mime: Some(expected_mime),
                    found_mime: Some(found_mime),
                })
            } else {
                Ok(())
            }
        }
    }
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

    fn req_content_type_smithy(content_type: &'static str) -> SmithyHeaders {
        let mut headers = SmithyHeaders::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_str(content_type).unwrap());
        headers
    }

    fn req_accept(accept: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static(accept));
        headers
    }

    const APPLICATION_JSON: Option<&'static str> = Some("application/json");

    // Validates the rejection type since we cannot implement `PartialEq`
    // for `MissingContentTypeReason`.
    fn assert_unexpected_mime_type(
        result: Result<(), MissingContentTypeReason>,
        actually_expected_mime: Option<mime::Mime>,
        actually_found_mime: Option<mime::Mime>,
    ) {
        match result {
            Ok(()) => panic!("content-type validation is expected to fail"),
            Err(e) => match e {
                MissingContentTypeReason::UnexpectedMimeType {
                    expected_mime,
                    found_mime,
                } => {
                    assert_eq!(actually_expected_mime, expected_mime);
                    assert_eq!(actually_found_mime, found_mime);
                }
                _ => panic!("unexpected `MissingContentTypeReason`: {}", e),
            },
        }
    }

    #[test]
    fn check_valid_content_type() {
        let headers = req_content_type_smithy("application/json");
        assert!(content_type_header_classifier_smithy(&headers, APPLICATION_JSON,).is_ok());
    }

    #[test]
    fn check_invalid_content_type() {
        let invalid = vec!["application/jason", "text/xml"];
        for invalid_mime in invalid {
            let headers = req_content_type_smithy(invalid_mime);
            let results = vec![content_type_header_classifier_smithy(&headers, APPLICATION_JSON)];

            let actually_expected_mime = Some(parse_mime(APPLICATION_JSON.unwrap()).unwrap());
            for result in results {
                let actually_found_mime = invalid_mime.parse::<mime::Mime>().ok();
                assert_unexpected_mime_type(result, actually_expected_mime.clone(), actually_found_mime);
            }
        }
    }

    #[test]
    fn check_missing_content_type_is_not_allowed() {
        let actually_expected_mime = Some(parse_mime(APPLICATION_JSON.unwrap()).unwrap());
        let result = content_type_header_classifier_smithy(&SmithyHeaders::new(), APPLICATION_JSON);
        assert_unexpected_mime_type(result, actually_expected_mime, None);
    }

    #[test]
    fn check_missing_content_type_is_expected() {
        let headers = req_content_type_smithy(APPLICATION_JSON.unwrap());
        let actually_found_mime = Some(parse_mime(APPLICATION_JSON.unwrap()).unwrap());
        let actually_expected_mime = None;

        let result = content_type_header_classifier_smithy(&headers, None);
        assert_unexpected_mime_type(result, actually_expected_mime, actually_found_mime);
    }

    #[test]
    fn check_not_parsable_content_type() {
        let request = req_content_type_smithy("123");
        let result = content_type_header_classifier_smithy(&request, APPLICATION_JSON);
        assert!(matches!(
            result.unwrap_err(),
            MissingContentTypeReason::MimeParseError(_)
        ));
    }

    #[test]
    fn check_non_ascii_visible_characters_content_type() {
        // Note that for Smithy headers, validation fails when attempting to parse the mime type,
        // unlike with `http`'s `HeaderMap`, that would fail when checking the header value is
        // valid (~ASCII string).
        let request = req_content_type_smithy("application/💩");
        let result = content_type_header_classifier_smithy(&request, APPLICATION_JSON);
        assert!(matches!(
            result.unwrap_err(),
            MissingContentTypeReason::MimeParseError(_)
        ));
    }

    #[test]
    fn valid_content_type_header_classifier_http_params() {
        let request = req_content_type_smithy("application/json; charset=utf-8");
        let result = content_type_header_classifier_smithy(&request, APPLICATION_JSON);
        assert!(result.is_ok());
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
