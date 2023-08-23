/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Functions shared between the tests of several modules.

use crate::http_request::{SignableBody, SignableRequest};
use http::{Method, Request, Uri};
use std::error::Error as StdError;

fn path(name: &str, ext: &str) -> String {
    format!("aws-sig-v4-test-suite/{}/{}.{}", name, name, ext)
}

fn read(path: &str) -> String {
    println!("Loading `{}` for test case...", path);
    match std::fs::read_to_string(path) {
        // This replacement is necessary for tests to pass on Windows, as reading the
        // sigv4 snapshots from the file system results in CRLF line endings being inserted.
        Ok(value) => value.replace("\r\n", "\n"),
        Err(err) => {
            panic!("failed to load test case `{}`: {}", path, err);
        }
    }
}

pub(crate) fn test_canonical_request(name: &str) -> String {
    // Tests fail if there's a trailing newline in the file, and pre-commit requires trailing newlines
    read(&path(name, "creq")).trim().to_string()
}

pub(crate) fn test_sts(name: &str) -> String {
    read(&path(name, "sts"))
}

pub(crate) fn test_request(name: &str) -> TestRequest {
    test_parsed_request(name, "req")
}

pub(crate) fn test_signed_request(name: &str) -> TestRequest {
    test_parsed_request(name, "sreq")
}

pub(crate) fn test_signed_request_query_params(name: &str) -> TestRequest {
    test_parsed_request(name, "qpsreq")
}

fn test_parsed_request(name: &str, ext: &str) -> TestRequest {
    let path = path(name, ext);
    match parse_request(read(&path).as_bytes()) {
        Ok(parsed) => parsed,
        Err(err) => panic!("Failed to parse {}: {}", path, err),
    }
}

pub(crate) struct TestRequest {
    pub(crate) uri: String,
    pub(crate) method: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: TestSignedBody,
}

pub(crate) enum TestSignedBody {
    Signable(SignableBody<'static>),
    Bytes(Vec<u8>),
}

impl TestSignedBody {
    fn as_signable_body(&self) -> SignableBody<'_> {
        match self {
            TestSignedBody::Signable(data) => data.clone(),
            TestSignedBody::Bytes(data) => SignableBody::Bytes(data.as_slice()),
        }
    }
}

impl TestRequest {
    pub(crate) fn set_body(&mut self, body: SignableBody<'static>) {
        self.body = TestSignedBody::Signable(body);
    }

    pub(crate) fn as_http_request(&self) -> http::Request<&'static str> {
        let mut builder = http::Request::builder()
            .uri(&self.uri)
            .method(Method::from_bytes(self.method.as_bytes()).unwrap());
        for (k, v) in &self.headers {
            builder = builder.header(k, v);
        }
        builder.body("body").unwrap()
    }
}

impl<B: AsRef<[u8]>> From<http::Request<B>> for TestRequest {
    fn from(value: Request<B>) -> Self {
        let invalid = value
            .headers()
            .values()
            .find(|h| std::str::from_utf8(h.as_bytes()).is_err());
        if let Some(invalid) = invalid {
            panic!("invalid header: {:?}", invalid);
        }
        Self {
            uri: value.uri().to_string(),
            method: value.method().to_string(),
            headers: value
                .headers()
                .iter()
                .map(|(k, v)| {
                    (
                        k.to_string(),
                        String::from_utf8(v.as_bytes().to_vec()).unwrap(),
                    )
                })
                .collect::<Vec<_>>(),
            body: TestSignedBody::Bytes(value.body().as_ref().to_vec()),
        }
    }
}

impl<'a> From<&'a TestRequest> for SignableRequest<'a> {
    fn from(request: &'a TestRequest) -> SignableRequest<'a> {
        SignableRequest::new(
            &request.method,
            &request.uri,
            request
                .headers
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str())),
            request.body.as_signable_body(),
        )
        .expect("URI MUST be valid")
    }
}

fn parse_request(s: &[u8]) -> Result<TestRequest, Box<dyn StdError + Send + Sync + 'static>> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    // httparse 1.5 requires two trailing newlines to head the header section.
    let mut with_newline = Vec::from(s);
    with_newline.push(b'\n');
    let mut req = httparse::Request::new(&mut headers);
    let _ = req.parse(&with_newline).unwrap();

    let mut uri_builder = Uri::builder().scheme("https");
    if let Some(path) = req.path {
        uri_builder = uri_builder.path_and_query(path);
    }

    let mut headers = vec![];
    for header in req.headers {
        let name = header.name.to_lowercase();
        if name == "host" {
            uri_builder = uri_builder.authority(header.value);
        } else if !name.is_empty() {
            headers.push((
                header.name.to_string(),
                std::str::from_utf8(header.value)?.to_string(),
            ));
        }
    }

    Ok(TestRequest {
        uri: uri_builder.build()?.to_string(),
        method: req.method.unwrap().to_string(),
        headers,
        body: TestSignedBody::Bytes(vec![]),
    })
}

#[test]
fn test_parse_headers() {
    let buf = b"Host:example.amazonaws.com\nX-Amz-Date:20150830T123600Z\n\nblah blah";
    let mut headers = [httparse::EMPTY_HEADER; 4];
    assert_eq!(
        httparse::parse_headers(buf, &mut headers),
        Ok(httparse::Status::Complete((
            56,
            &[
                httparse::Header {
                    name: "Host",
                    value: b"example.amazonaws.com",
                },
                httparse::Header {
                    name: "X-Amz-Date",
                    value: b"20150830T123600Z",
                }
            ][..]
        )))
    );
}

#[test]
fn test_parse() {
    test_request("post-header-key-case");
}

#[test]
fn test_read_query_params() {
    test_request("get-vanilla-query-order-key-case");
}
