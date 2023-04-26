/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Protocol helpers.
use crate::rejection::{MissingContentTypeReason, NotAcceptableReason};
use http::{HeaderMap, HeaderValue};

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

pub fn accept_header_classifier(headers: &HeaderMap, content_type: &'static str) -> Result<(), NotAcceptableReason> {
    if !headers.contains_key(http::header::ACCEPT) {
        return Ok(());
    }
    // Must be of the form: `type/subtype`.
    let content_type = content_type
        .parse::<mime::Mime>()
        // `expect` safety: content_type` is sent in from the generated server SDK and we know it's valid.
        .expect("MIME parsing failed, `content_type` is not valid; please file a bug report under https://github.com/awslabs/smithy-rs/issues");
    let accept_headers = headers.get_all(http::header::ACCEPT);
    let can_satisfy_some_accept_header = accept_headers
        .iter()
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
                .flat_map(|s| s.split(',').map(|type_| type_.split(';').next().unwrap().trim()))
        })
        .filter_map(|h| h.parse::<mime::Mime>().ok())
        .any(|mime| {
            let type_ = content_type.type_();
            let subtype = content_type.subtype();
            // Accept: */*, type/*, type/subtype
            match (mime.type_(), mime.subtype()) {
                (t, s) if t == type_ && s == subtype => true,
                (t, mime::STAR) if t == type_ => true,
                (mime::STAR, mime::STAR) => true,
                _ => false,
            }
        });
    if can_satisfy_some_accept_header {
        Ok(())
    } else {
        // We can't make `NotAcceptableReason`/`RequestRejection` borrow the header values because
        // non-static lifetimes are not allowed in the source of an error, because
        // `std::error::Error` requires the source is `dyn Error + 'static`. So we clone them into
        // a vector in the case of a request rejection.
        let cloned_accept_headers: Vec<HeaderValue> = accept_headers.into_iter().cloned().collect();
        Err(NotAcceptableReason::CannotSatisfyAcceptHeaders(cloned_accept_headers))
    }
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

    fn req_accept(accept: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_str(accept).unwrap());
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

    mod accept_header_classifier {
        use super::*;

        #[test]
        fn valid_single_value() {
            let valid_request = req_accept("application/json");
            assert!(accept_header_classifier(&valid_request, "application/json").is_ok());
        }

        #[test]
        fn valid_multiple_values() {
            let valid_request = req_accept("text/strings, application/json, invalid");
            assert!(accept_header_classifier(&valid_request, "application/json").is_ok());
        }

        #[test]
        fn subtype_star() {
            let valid_request = req_accept("application/*");
            assert!(accept_header_classifier(&valid_request, "application/json").is_ok());
        }

        #[test]
        fn type_star_subtype_star() {
            let valid_request = req_accept("*/*");
            assert!(accept_header_classifier(&valid_request, "application/json").is_ok());
        }

        #[test]
        fn empty() {
            assert!(accept_header_classifier(&HeaderMap::new(), "application/json").is_ok());
        }

        #[test]
        fn valid_with_params() {
            let valid_request = req_accept("application/json; q=30, */*");
            assert!(accept_header_classifier(&valid_request, "application/json").is_ok());
        }

        #[test]
        fn unstatisfiable_multiple_values() {
            let accept_header_values = ["text/invalid, invalid, invalid/invalid"];
            let joined = accept_header_values.join(", ");
            let invalid_request = req_accept(&joined);
            match accept_header_classifier(&invalid_request, "application/json").unwrap_err() {
                NotAcceptableReason::CannotSatisfyAcceptHeaders(returned_accept_header_values) => {
                    for header_value in accept_header_values {
                        let header_value = HeaderValue::from_str(header_value).unwrap();
                        assert!(returned_accept_header_values.contains(&header_value));
                    }
                }
            }
        }

        #[test]
        fn unstatisfiable_unparseable() {
            let header_value = "foo_"; // Not a valid MIME type.
            assert!(header_value.parse::<mime::Mime>().is_err());
            let invalid_request = req_accept(header_value);
            match accept_header_classifier(&invalid_request, "application/json").unwrap_err() {
                NotAcceptableReason::CannotSatisfyAcceptHeaders(returned_accept_header_values) => {
                    let header_value = HeaderValue::from_str(header_value).unwrap();
                    assert!(returned_accept_header_values.contains(&header_value));
                }
            }
        }

        #[test]
        #[should_panic]
        fn panic_if_content_type_not_parseable() {
            let header_value = "foo_"; // Not a valid MIME type.
            let mut headers = HeaderMap::new();
            headers.insert(ACCEPT, HeaderValue::from_str(header_value).unwrap());
            assert!(header_value.parse::<mime::Mime>().is_err());
            let _res = accept_header_classifier(&headers, header_value);
        }
    }
}
