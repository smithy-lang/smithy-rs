/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Protocol helpers.
use crate::rejection::{ContentTypeRejection, MimeParsingFailed, MissingJsonContentType, MissingXmlContentType};
use axum_core::extract::RequestParts;

/// Validate that the request had the standard JSON content-type header.
pub fn check_json_content_type<B>(req: &RequestParts<B>) -> Result<(), ContentTypeRejection> {
    let mime = req
        .headers()
        .ok_or(MissingJsonContentType)?
        .get(http::header::CONTENT_TYPE)
        .ok_or(MissingJsonContentType)?
        .to_str()
        .map_err(|_| MissingJsonContentType)?
        .parse::<mime::Mime>()
        .map_err(|_| MimeParsingFailed)?;

    if mime.type_() == "application"
        && (mime.subtype() == "json" || mime.suffix().filter(|name| *name == "json").is_some())
    {
        Ok(())
    } else {
        Err(ContentTypeRejection::MissingJsonContentType(MissingJsonContentType))
    }
}

/// Validate that the request had the standard XML content-type header.
pub fn check_xml_content_type<B>(req: &RequestParts<B>) -> Result<(), ContentTypeRejection> {
    let mime = req
        .headers()
        .ok_or(MissingXmlContentType)?
        .get(http::header::CONTENT_TYPE)
        .ok_or(MissingXmlContentType)?
        .to_str()
        .map_err(|_| MissingXmlContentType)?
        .parse::<mime::Mime>()
        .map_err(|_| MimeParsingFailed)?;

    if mime.type_() == "application"
        && (mime.subtype() == "xml" || mime.suffix().filter(|name| *name == "xml").is_some())
    {
        Ok(())
    } else {
        Err(ContentTypeRejection::MissingXmlContentType(MissingXmlContentType))
    }
}
