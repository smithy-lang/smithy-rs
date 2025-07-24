/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Functions shared between the tests of several modules.

use crate::http_request::canonical_request::{CanonicalRequest, StringToSign};
use crate::http_request::{
    PayloadChecksumKind, SessionTokenMode, SignableBody, SignableRequest, SignatureLocation,
    SigningSettings,
};
use aws_credential_types::Credentials;
use aws_smithy_runtime_api::client::identity::Identity;
use http0::{Method, Uri};
use std::error::Error as StdError;
use std::time::{Duration, SystemTime};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub(crate) mod v4 {
    use super::*;

    fn path(name: &str, ext: &str) -> String {
        format!("aws-sig-v4-test-suite/{}/{}.{}", name, name, ext)
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

    #[test]
    fn test_parse() {
        test_request("post-header-key-case");
    }

    #[test]
    fn test_read_query_params() {
        test_request("get-vanilla-query-order-key-case");
    }
}

/// Common test suite collection
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Collection {
    V4,
    V4A,
}

/// A test from the common CRT test suite
#[derive(Debug, Clone)]
pub(crate) struct SigningSuiteTest {
    test_name: &'static str,
    collection: Collection,
}

impl SigningSuiteTest {
    /// Create a new test from the V4 test suite
    fn v4(test_name: &'static str) -> Self {
        Self {
            test_name,
            collection: Collection::V4,
        }
    }

    /// Create a new test from the V4a test suite
    fn v4a(test_name: &'static str) -> Self {
        Self {
            test_name,
            collection: Collection::V4A,
        }
    }

    /// Get the path to a file in this test suite directory
    fn path(&self, filename: &str) -> String {
        let dir = match self.collection {
            Collection::V4 => "v4",
            Collection::V4A => "v4a",
        };
        format!("aws-signing-test-suite/{dir}/{}/{filename}", self.test_name)
    }

    /// Get the HTTP request for the test
    pub(crate) fn request(&self) -> TestRequest {
        test_parsed_request(&self.path("request.txt"))
    }

    /// Get the canonical request for the test
    pub(crate) fn canonical_request(&self, signature_location: SignatureLocation) -> String {
        match signature_location {
            SignatureLocation::QueryParams => read(&self.path("query-canonical-request.txt")),
            SignatureLocation::Headers => read(&self.path("header-canonical-request.txt")),
        }
    }

    /// Get the string to sign for the test
    pub(crate) fn string_to_sign(&self, signature_location: SignatureLocation) -> String {
        match signature_location {
            SignatureLocation::QueryParams => read(&self.path("query-string-to-sign.txt")),
            SignatureLocation::Headers => read(&self.path("header-string-to-sign.txt")),
        }
    }

    /// Get the signature for the test
    pub(crate) fn signature(&self, signature_location: SignatureLocation) -> String {
        match signature_location {
            SignatureLocation::QueryParams => read(&self.path("query-signature.txt")),
            SignatureLocation::Headers => read(&self.path("header-signature.txt")),
        }
    }

    /// Get the test context for the test
    pub(crate) fn context(&self) -> TestContext {
        let context = read(&self.path("context.json"));
        let tc_builder: TestContextBuilder = serde_json::from_str(&context).unwrap();
        tc_builder.build()
    }
}

fn test_parsed_request(path: &str) -> TestRequest {
    match parse_request(read(path).as_bytes()) {
        Ok(parsed) => parsed,
        Err(err) => panic!("Failed to parse {}: {}", path, err),
    }
}

fn new_v4_signing_params_from_context(
    test_context: &'_ TestContext,
    signature_location: SignatureLocation,
) -> crate::http_request::SigningParams<'_> {
    let mut params = crate::sign::v4::SigningParams::from(test_context);
    params.settings.signature_location = signature_location;
    params.into()
}

pub(crate) fn run_test_suite_v4(test_name: &'static str) {
    run_v4_test(test_name, SignatureLocation::Headers);
    run_v4_test(test_name, SignatureLocation::QueryParams);
}

fn run_v4_test(test_name: &'static str, signature_location: SignatureLocation) {
    let test = SigningSuiteTest::v4(test_name);
    let tc = test.context();
    let params = new_v4_signing_params_from_context(&tc, signature_location);

    let req = test.request();
    let expected_creq = test.canonical_request(signature_location);
    let signable_req = SignableRequest::from(&req);
    let actual_creq = CanonicalRequest::from(&signable_req, &params).unwrap();

    // check canonical request
    assert_eq!(
        expected_creq,
        actual_creq.to_string(),
        "canonical request didn't match (signature location: {signature_location:?})"
    );

    let expected_string_to_sign = test.string_to_sign(signature_location);
    let hashed_creq = &crate::sign::v4::sha256_hex_string(actual_creq.to_string().as_bytes());
    let actual_string_to_sign = StringToSign::new_v4(
        *params.time(),
        params.region().unwrap(),
        params.name(),
        hashed_creq,
    )
    .to_string();

    // check string to sign
    assert_eq!(
        expected_string_to_sign, actual_string_to_sign,
        "'string to sign' didn't match (signature location: {signature_location:?})"
    );

    let out = crate::http_request::sign(signable_req, &params).unwrap();
    out.output
        .apply_to_request_http0x(&mut req.as_http_request());

    // check signature
    assert_eq!(
        test.signature(signature_location),
        out.signature,
        "signature didn't match (signature location: {signature_location:?})"
    );

    // TODO check signed request
}

/// Test suite context.json
pub(crate) struct TestContext {
    pub(crate) identity: Identity,
    pub(crate) expiration_in_seconds: u64,
    pub(crate) normalize: bool,
    pub(crate) region: String,
    pub(crate) service: String,
    pub(crate) timestamp: String,
    pub(crate) omit_session_token: bool,
    pub(crate) sign_body: bool,
}

// Serde has limitations requiring this odd workaround.
// See https://github.com/serde-rs/serde/issues/368 for more info.
fn return_true() -> bool {
    true
}

#[derive(serde_derive::Deserialize)]
pub(crate) struct TestContextBuilder {
    credentials: TestContextCreds,
    expiration_in_seconds: u64,
    normalize: bool,
    region: String,
    service: String,
    timestamp: String,
    #[serde(default)]
    omit_session_token: bool,
    #[serde(default = "return_true")]
    sign_body: bool,
}

impl TestContextBuilder {
    pub(crate) fn build(self) -> TestContext {
        let identity = Identity::new(
            Credentials::from_keys(
                &self.credentials.access_key_id,
                &self.credentials.secret_access_key,
                self.credentials.token.clone(),
            ),
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(self.expiration_in_seconds)),
        );

        TestContext {
            identity,
            expiration_in_seconds: self.expiration_in_seconds,
            normalize: self.normalize,
            region: self.region,
            service: self.service,
            timestamp: self.timestamp,
            omit_session_token: self.omit_session_token,
            sign_body: self.sign_body,
        }
    }
}

#[derive(serde_derive::Deserialize)]
pub(crate) struct TestContextCreds {
    access_key_id: String,
    secret_access_key: String,
    token: Option<String>,
}

impl<'a> From<&'a TestContext> for crate::sign::v4::SigningParams<'a, SigningSettings> {
    fn from(tc: &'a TestContext) -> Self {
        crate::sign::v4::SigningParams {
            identity: &tc.identity,
            region: &tc.region,
            name: &tc.service,
            time: OffsetDateTime::parse(&tc.timestamp, &Rfc3339)
                .unwrap()
                .into(),
            settings: SigningSettings {
                // payload_checksum_kind: PayloadChecksumKind::XAmzSha256,
                expires_in: Some(Duration::from_secs(tc.expiration_in_seconds)),
                uri_path_normalization_mode: tc.normalize.into(),
                session_token_mode: if tc.omit_session_token {
                    SessionTokenMode::Exclude
                } else {
                    SessionTokenMode::Include
                },
                payload_checksum_kind: if tc.sign_body {
                    PayloadChecksumKind::XAmzSha256
                } else {
                    PayloadChecksumKind::NoHeader
                },
                ..Default::default()
            },
        }
    }
}

#[cfg(feature = "sigv4a")]
pub(crate) mod v4a {
    use super::*;
    use crate::http_request::{
        sign, PayloadChecksumKind, SessionTokenMode, SignatureLocation, SigningSettings,
    };
    use crate::sign::v4a;
    use p256::ecdsa::signature::{Signature, Verifier};
    use p256::ecdsa::{DerSignature, SigningKey};
    use std::time::Duration;
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;

    fn new_v4a_signing_params_from_context(
        test_context: &'_ TestContext,
        signature_location: SignatureLocation,
    ) -> crate::http_request::SigningParams<'_> {
        let mut params = crate::sign::v4a::SigningParams::from(test_context);
        params.settings.signature_location = signature_location;
        params.into()
    }

    pub(crate) fn run_test_suite_v4a(test_name: &'static str) {
        run_v4a_test(test_name, SignatureLocation::Headers);
        run_v4a_test(test_name, SignatureLocation::QueryParams);
    }

    pub(crate) fn run_v4a_test(test_name: &'static str, signature_location: SignatureLocation) {
        let test = SigningSuiteTest::v4a(test_name);
        let tc = test.context();
        let params = new_v4a_signing_params_from_context(&tc, signature_location);

        let req = test.request();
        let expected_creq = test.canonical_request(signature_location);
        let signable_req = SignableRequest::from(&req);
        let actual_creq = CanonicalRequest::from(&signable_req, &params).unwrap();

        assert_eq!(
            expected_creq,
            actual_creq.to_string(),
            "canonical request didn't match (signature location: {signature_location:?})"
        );

        let expected_string_to_sign = test.string_to_sign(signature_location);
        let hashed_creq = &crate::sign::v4::sha256_hex_string(actual_creq.to_string().as_bytes());
        let actual_string_to_sign = StringToSign::new_v4a(
            *params.time(),
            params.region_set().unwrap(),
            params.name(),
            hashed_creq,
        )
        .to_string();

        assert_eq!(
            expected_string_to_sign, actual_string_to_sign,
            "'string to sign' didn't match (signature location: {signature_location:?})"
        );

        let out = sign(signable_req, &params).unwrap();
        // Sigv4a signatures are non-deterministic, so we can't compare the signature directly.
        out.output
            .apply_to_request_http0x(&mut req.as_http_request());

        let creds = params.credentials().unwrap();
        let signing_key =
            v4a::generate_signing_key(creds.access_key_id(), creds.secret_access_key());
        let sig = DerSignature::from_bytes(&hex::decode(out.signature).unwrap()).unwrap();
        let sig = sig
            .try_into()
            .expect("DER-style signatures are always convertible into fixed-size signatures");

        let signing_key = SigningKey::from_bytes(signing_key.as_ref()).unwrap();
        let peer_public_key = signing_key.verifying_key();
        let sts = actual_string_to_sign.as_bytes();
        peer_public_key.verify(sts, &sig).unwrap();
        // TODO - check what CRT/Kotlin verify here for v4a
        // TODO - public.key.json ?
    }

    impl<'a> From<&'a TestContext> for crate::sign::v4a::SigningParams<'a, SigningSettings> {
        fn from(tc: &'a TestContext) -> Self {
            crate::sign::v4a::SigningParams {
                identity: &tc.identity,
                region_set: &tc.region,
                name: &tc.service,
                time: OffsetDateTime::parse(&tc.timestamp, &Rfc3339)
                    .unwrap()
                    .into(),
                settings: SigningSettings {
                    // payload_checksum_kind: PayloadChecksumKind::XAmzSha256,
                    expires_in: Some(Duration::from_secs(tc.expiration_in_seconds)),
                    uri_path_normalization_mode: tc.normalize.into(),
                    session_token_mode: if tc.omit_session_token {
                        SessionTokenMode::Exclude
                    } else {
                        SessionTokenMode::Include
                    },
                    payload_checksum_kind: if tc.sign_body {
                        PayloadChecksumKind::XAmzSha256
                    } else {
                        PayloadChecksumKind::NoHeader
                    },
                    ..Default::default()
                },
            }
        }
    }

    #[test]
    fn test_parse() {
        let req = SigningSuiteTest::v4a("post-header-key-case").request();
        assert_eq!(req.method, "POST");
        assert_eq!(req.uri, "https://example.amazonaws.com/");
        assert!(req.headers.is_empty());
    }

    #[test]
    fn test_read_query_params() {
        let req = SigningSuiteTest::v4a("get-header-value-trim").request();
        assert_eq!(req.method, "GET");
        assert_eq!(req.uri, "https://example.amazonaws.com/");
        assert!(!req.headers.is_empty());
    }
}

fn read(path: &str) -> String {
    println!("Loading `{}` for test case...", path);
    let v = {
        match std::fs::read_to_string(path) {
            // This replacement is necessary for tests to pass on Windows, as reading the
            // test snapshots from the file system results in CRLF line endings being inserted.
            Ok(value) => value.replace("\r\n", "\n"),
            Err(err) => {
                panic!("failed to load test case `{}`: {}", path, err);
            }
        }
    };

    v.trim().to_string()
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

    pub(crate) fn as_http_request(&self) -> http0::Request<&'static str> {
        let mut builder = http0::Request::builder()
            .uri(&self.uri)
            .method(Method::from_bytes(self.method.as_bytes()).unwrap());
        for (k, v) in &self.headers {
            builder = builder.header(k, v);
        }
        builder.body("body").unwrap()
    }
}

impl<B: AsRef<[u8]>> From<http0::Request<B>> for TestRequest {
    fn from(value: http0::Request<B>) -> Self {
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
