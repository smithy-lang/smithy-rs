/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Protocol helpers.
use crate::exception;
use axum_core::extract::RequestParts;

#[derive(Debug)]
pub enum Protocol {
    RestJson1,
}

/// Validate that the request had the standard JSON content-type header.
pub fn check_json_content_type<B>(req: &RequestParts<B>) -> Result<(), exception::FromRequest> {
    let mime = req
        .headers()
        .ok_or(exception::FromRequest::MissingJsonContentType)?
        .get(http::header::CONTENT_TYPE)
        .ok_or(exception::FromRequest::MissingJsonContentType)?
        .to_str()
        .map_err(|_| exception::FromRequest::MissingJsonContentType)?
        .parse::<mime::Mime>()
        .map_err(|_| exception::FromRequest::MimeParse)?;

    if mime.type_() == "application"
        && (mime.subtype() == "json" || mime.suffix().filter(|name| *name == "json").is_some())
    {
        Ok(())
    } else {
        Err(exception::FromRequest::MissingJsonContentType)
    }
}

/// Validate that the request had the standard XML content-type header.
pub fn check_xml_content_type<B>(req: &RequestParts<B>) -> Result<(), exception::FromRequest> {
    let mime = req
        .headers()
        .ok_or(exception::FromRequest::MissingXmlContentType)?
        .get(http::header::CONTENT_TYPE)
        .ok_or(exception::FromRequest::MissingXmlContentType)?
        .to_str()
        .map_err(|_| exception::FromRequest::MissingXmlContentType)?
        .parse::<mime::Mime>()
        .map_err(|_| exception::FromRequest::MimeParse)?;

    if mime.type_() == "application"
        && (mime.subtype() == "xml" || mime.suffix().filter(|name| *name == "xml").is_some())
    {
        Ok(())
    } else {
        Err(exception::FromRequest::MissingXmlContentType)
    }
}
