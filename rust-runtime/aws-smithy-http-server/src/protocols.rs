/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Protocol helpers.
use crate::rejection::MissingContentTypeReason;
#[allow(deprecated)]
use crate::request::RequestParts;
use http::HeaderMap;

/// When there are no modeled inputs,
/// a request body is empty and the content-type request header must not be set
#[allow(deprecated)]
pub fn content_type_header_empty_body_no_modeled_input<B>(
    req: &RequestParts<B>,
) -> Result<(), MissingContentTypeReason> {
    if req.headers().is_none() {
        return Ok(());
    }
    let headers = req.headers().unwrap();
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

fn parse_content_type(headers: &HeaderMap) -> Result<mime::Mime, MissingContentTypeReason> {
    headers
        .get(http::header::CONTENT_TYPE)
        .unwrap() // The header is present, `unwrap` will not panic.
        .to_str()
        .map_err(MissingContentTypeReason::ToStrError)?
        .parse::<mime::Mime>()
        .map_err(MissingContentTypeReason::MimeParseError)
}

/// Checks that the content-type in request headers is valid
#[allow(deprecated)]
pub fn content_type_header_classifier<B>(
    req: &RequestParts<B>,
    expected_content_type: Option<&'static str>,
) -> Result<(), MissingContentTypeReason> {
    // Allow no CONTENT-TYPE header
    if req.headers().is_none() {
        return Ok(());
    }
    let headers = req.headers().unwrap(); // Headers are present, `unwrap` will not panic.
    if !headers.contains_key(http::header::CONTENT_TYPE) {
        return Ok(());
    }
    let found_mime = parse_content_type(headers)?;
    // There is a content-type header
    // If there is an implied content type, they must match
    if let Some(expected_content_type) = expected_content_type {
        let expected_mime = expected_content_type
            .parse::<mime::Mime>()
            // `expected_content_type` comes from the codegen.
            .expect("BUG: MIME parsing failed, expected_content_type is not valid. Please file a bug report under https://github.com/awslabs/smithy-rs/issues");
        if expected_content_type != found_mime {
            return Err(MissingContentTypeReason::UnexpectedMimeType {
                expected_mime: Some(expected_mime),
                found_mime: Some(found_mime),
            });
        }
    } else {
        // Content-type header and no modeled input (mismatch)
        return Err(MissingContentTypeReason::UnexpectedMimeType {
            expected_mime: None,
            found_mime: Some(found_mime),
        });
    }
    Ok(())
}

#[allow(deprecated)]
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

#[allow(deprecated)]
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

    const EXPECTED_MIME_APPLICATION_JSON: Option<&'static str> = Some("application/json");

    #[test]
    fn check_content_type_header_empty_body_no_modeled_input() {
        let request = Request::builder().body("").unwrap();
        let request = RequestParts::new(request);
        assert!(content_type_header_empty_body_no_modeled_input(&request).is_ok());
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
        let request = RequestParts::new(Request::builder().body("").unwrap());
        let result = content_type_header_classifier(&request, EXPECTED_MIME_APPLICATION_JSON);
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
