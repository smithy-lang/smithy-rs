/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Protocol helpers.
use crate::rejection::MissingContentTypeReason;
use crate::request::RequestParts;

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
        .map_err(|e| MissingContentTypeReason::ToStrError(e))?
        .parse::<mime::Mime>()
        .map_err(|e| MissingContentTypeReason::MimeParseError(e))?;
    if &found_mime == expected_mime {
        Ok(())
    } else {
        Err(MissingContentTypeReason::UnexpectedMimeType {
            expected_mime,
            found_mime,
        })
    }
}

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

    static EXPECTED_MIME_APPLICATION_JSON: once_cell::sync::Lazy<mime::Mime> =
        once_cell::sync::Lazy::new(|| "application/json".parse::<mime::Mime>().unwrap());

    #[test]
    fn check_valid_content_type() {
        let valid_request = req("application/json");
        assert!(check_content_type(&valid_request, &EXPECTED_MIME_APPLICATION_JSON).is_ok());
    }

    #[test]
    fn check_invalid_content_type() {
        let invalid = vec!["application/ajson", "text/xml"];
        for invalid_mime in invalid {
            let request = req(invalid_mime);
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
        let request = req("123");
        let result = check_content_type(&request, &EXPECTED_MIME_APPLICATION_JSON);
        assert!(matches!(
            result.unwrap_err(),
            MissingContentTypeReason::MimeParseError(_)
        ));
    }

    #[test]
    fn check_non_ascii_visible_characters_content_type() {
        let request = req("application/💩");
        let result = check_content_type(&request, &EXPECTED_MIME_APPLICATION_JSON);
        assert!(matches!(result.unwrap_err(), MissingContentTypeReason::ToStrError(_)));
    }
}
