/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Protocol helpers.
use crate::rejection::RequestRejection;
use axum_core::extract::RequestParts;

#[derive(Debug)]
pub enum Protocol {
    RestJson1,
}

/// Validate that the request had the standard JSON content-type header.
pub fn check_json_content_type<B>(req: &RequestParts<B>) -> Result<(), RequestRejection> {
    let mime = req
        .headers()
        .ok_or(RequestRejection::MissingJsonContentType)?
        .get(http::header::CONTENT_TYPE)
        .ok_or(RequestRejection::MissingJsonContentType)?
        .to_str()
        .map_err(|_| RequestRejection::MissingJsonContentType)?
        .parse::<mime::Mime>()
        .map_err(|_| RequestRejection::MimeParse)?;

    if mime.type_() == "application"
        && (mime.subtype() == "json" || mime.suffix().filter(|name| *name == "json").is_some())
    {
        Ok(())
    } else {
        Err(RequestRejection::MissingJsonContentType)
    }
}

/// Validate that the request had the standard XML content-type header.
pub fn check_xml_content_type<B>(req: &RequestParts<B>) -> Result<(), RequestRejection> {
    let mime = req
        .headers()
        .ok_or(RequestRejection::MissingXmlContentType)?
        .get(http::header::CONTENT_TYPE)
        .ok_or(RequestRejection::MissingXmlContentType)?
        .to_str()
        .map_err(|_| RequestRejection::MissingXmlContentType)?
        .parse::<mime::Mime>()
        .map_err(|_| RequestRejection::MimeParse)?;

    if mime.type_() == "application"
        && (mime.subtype() == "xml" || mime.suffix().filter(|name| *name == "xml").is_some())
    {
        Ok(())
    } else {
        Err(RequestRejection::MissingXmlContentType)
    }
}
