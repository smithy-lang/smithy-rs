/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Protocol helpers.
use crate::rejection::RequestRejection;
use axum_core::extract::RequestParts;

/// Smithy protocols enumerator.
#[derive(Debug, Clone, Copy)]
pub enum Protocol {
    RestJson1,
    RestXml,
    AwsJson10,
    AwsJson11,
}

/// TODO: This check should be more strict and codegenerated.
/// Validate that the request had the standard RestJson 1 content-type header.
pub fn check_rest_json_1_content_type<B>(req: &RequestParts<B>) -> Result<(), RequestRejection> {
    let mime = req
        .headers()
        .ok_or(RequestRejection::MissingRestJson1ContentType)?
        .get(http::header::CONTENT_TYPE)
        .ok_or(RequestRejection::MissingRestJson1ContentType)?
        .to_str()
        .map_err(|_| RequestRejection::MissingRestJson1ContentType)?
        .parse::<mime::Mime>()
        .map_err(|_| RequestRejection::MimeParse)?;

    if mime.type_() == "application"
        && (mime.subtype() == "json" || mime.suffix().filter(|name| *name == "json").is_some())
    {
        Ok(())
    } else {
        Err(RequestRejection::MissingRestJson1ContentType)
    }
}

/// TODO: This check should be more strict and codegenerated.
/// Validate that the request had the standard XML content-type header.
pub fn check_rest_xml_content_type<B>(req: &RequestParts<B>) -> Result<(), RequestRejection> {
    let mime = req
        .headers()
        .ok_or(RequestRejection::MissingRestXmlContentType)?
        .get(http::header::CONTENT_TYPE)
        .ok_or(RequestRejection::MissingRestXmlContentType)?
        .to_str()
        .map_err(|_| RequestRejection::MissingRestXmlContentType)?
        .parse::<mime::Mime>()
        .map_err(|_| RequestRejection::MimeParse)?;

    if mime.type_() == "application"
        && (mime.subtype() == "xml" || mime.suffix().filter(|name| *name == "xml").is_some())
    {
        Ok(())
    } else {
        Err(RequestRejection::MissingRestXmlContentType)
    }
}

/// TODO: This check should be more strict and codegenerated.
/// Validate that the request had the standard AwsJson 1.0 content-type header.
pub fn check_aws_json_10_content_type<B>(req: &RequestParts<B>) -> Result<(), RequestRejection> {
    let mime = req
        .headers()
        .ok_or(RequestRejection::MissingAwsJson10ContentType)?
        .get(http::header::CONTENT_TYPE)
        .ok_or(RequestRejection::MissingAwsJson10ContentType)?
        .to_str()
        .map_err(|_| RequestRejection::MissingAwsJson10ContentType)?
        .parse::<mime::Mime>()
        .map_err(|_| RequestRejection::MimeParse)?;

    if mime.type_() == "application" && mime.subtype() == "x-amz-json-1.0" {
        Ok(())
    } else {
        Err(RequestRejection::MissingAwsJson10ContentType)
    }
}

/// TODO: This check should be more strict and codegenerated.
/// Validate that the request had the standard AwsJson 1.1 content-type header.
pub fn check_aws_json_11_content_type<B>(req: &RequestParts<B>) -> Result<(), RequestRejection> {
    let mime = req
        .headers()
        .ok_or(RequestRejection::MissingAwsJson11ContentType)?
        .get(http::header::CONTENT_TYPE)
        .ok_or(RequestRejection::MissingAwsJson11ContentType)?
        .to_str()
        .map_err(|_| RequestRejection::MissingAwsJson11ContentType)?
        .parse::<mime::Mime>()
        .map_err(|_| RequestRejection::MimeParse)?;

    if mime.type_() == "application" && mime.subtype() == "x-amz-json-1.1" {
        Ok(())
    } else {
        Err(RequestRejection::MissingAwsJson11ContentType)
    }
}
