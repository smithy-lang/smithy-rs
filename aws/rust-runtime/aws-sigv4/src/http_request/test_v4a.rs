/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Functions shared between the tests of several modules.

use super::SignatureLocation;
use bytes::Bytes;
use http::{Method, Request, Uri, Version};
use serde_derive::Deserialize;
use std::error::Error as StdError;

fn path(test_name: &str, definition_name: &str) -> String {
    format!("aws-sig-v4a-test-suite/{test_name}/{definition_name}.txt")
}

fn read(path: &str) -> String {
    println!("Loading `{}` for test case...", path);
    match std::fs::read_to_string(path) {
        // This replacement is necessary for tests to pass on Windows, as reading the
        // sigv4 snapshots from the file system results in CRLF line endings being inserted.
        Ok(value) => value.replace("\r\n", "\n").trim().to_string(),
        Err(err) => {
            panic!("failed to load test case `{}`: {}", path, err);
        }
    }
}

pub(crate) fn test_request(name: &str) -> Request<Bytes> {
    test_parsed_request(&path(name, "request"))
}

pub(crate) fn test_canonical_request(name: &str, signature_location: SignatureLocation) -> String {
    match signature_location {
        SignatureLocation::QueryParams => read(&path(name, "query-canonical-request")),
        SignatureLocation::Headers => read(&path(name, "header-canonical-request")),
    }
}

pub(crate) fn test_string_to_sign(name: &str, signature_location: SignatureLocation) -> String {
    match signature_location {
        SignatureLocation::QueryParams => read(&path(name, "query-string-to-sign")),
        SignatureLocation::Headers => read(&path(name, "header-string-to-sign")),
    }
}

fn test_parsed_request(path: &str) -> Request<Bytes> {
    match parse_request(read(path).as_bytes()) {
        Ok(parsed) => parsed,
        Err(err) => panic!("Failed to parse {}: {}", path, err),
    }
}

fn parse_request(s: &[u8]) -> Result<Request<Bytes>, Box<dyn StdError + Send + Sync + 'static>> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    // httparse 1.5 requires two trailing newlines to head the header section.
    let mut with_newline = Vec::from(s);
    with_newline.push(b'\n');
    let mut req = httparse::Request::new(&mut headers);
    let _ = req.parse(&with_newline).unwrap();

    let version = match req.version.unwrap() {
        1 => Version::HTTP_11,
        _ => unimplemented!(),
    };

    let method = match req.method.unwrap() {
        "GET" => Method::GET,
        "POST" => Method::POST,
        _ => unimplemented!(),
    };

    let mut builder = Request::builder();
    builder = builder.version(version);
    builder = builder.method(method);

    let mut uri_builder = Uri::builder().scheme("https");
    if let Some(path) = req.path {
        uri_builder = uri_builder.path_and_query(path);
    }
    for header in req.headers {
        let name = header.name.to_lowercase();
        if name == "host" {
            uri_builder = uri_builder.authority(header.value);
        } else if !name.is_empty() {
            builder = builder.header(&name, header.value);
        }
    }

    builder = builder.uri(uri_builder.build()?);
    let req = builder.body(Bytes::new())?;
    Ok(req)
}

pub(crate) fn test_context(test_name: &str) -> TestContext {
    let path = format!("aws-sig-v4a-test-suite/{test_name}/context.json");
    let context = read(&path);
    serde_json::from_str(&context).unwrap()
}

// I can't figure out how to default to `true` otherwise. Please help :(
fn why_serde_why() -> bool {
    true
}

#[derive(Deserialize)]
pub(crate) struct TestContext {
    pub(crate) credentials: TestContextCreds,
    pub(crate) expiration_in_seconds: u64,
    pub(crate) normalize: bool,
    pub(crate) region: String,
    pub(crate) service: String,
    pub(crate) timestamp: String,
    #[serde(default)]
    pub(crate) omit_session_token: bool,
    #[serde(default = "why_serde_why")]
    pub(crate) sign_body: bool,
}

#[derive(Deserialize)]
pub(crate) struct TestContextCreds {
    pub(crate) access_key_id: String,
    pub(crate) secret_access_key: String,
    pub(crate) token: Option<String>,
}
