/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::error::SigningError;
use super::{PayloadChecksumKind, SignatureLocation};
use crate::http_request::canonical_request::header;
use crate::http_request::canonical_request::param;
use crate::http_request::canonical_request::{CanonicalRequest, StringToSign, HMAC_256};
use crate::http_request::error::CanonicalRequestError;
use crate::http_request::SigningParams;
use crate::sign::{calculate_signature, generate_signing_key, sha256_hex_string};
use crate::SigningOutput;

use http::Uri;
use std::borrow::Cow;

use std::fmt::{Debug, Formatter};
use std::str;

/// Represents all of the information necessary to sign an HTTP request.
#[derive(Debug)]
#[non_exhaustive]
pub struct SignableRequest<'a> {
    method: &'a str,
    uri: Uri,
    headers: Vec<(&'a str, &'a str)>,
    body: SignableBody<'a>,
}

impl<'a> SignableRequest<'a> {
    /// Creates a new `SignableRequest`.
    pub fn new(
        method: &'a str,
        uri: impl Into<Cow<'a, str>>,
        headers: impl Iterator<Item = (&'a str, &'a str)>,
        body: SignableBody<'a>,
    ) -> Result<Self, SigningError> {
        let uri = uri
            .into()
            .parse()
            .map_err(|e| SigningError::from(CanonicalRequestError::from(e)))?;
        let headers = headers.collect();
        Ok(Self {
            method,
            uri,
            headers,
            body,
        })
    }

    /// Returns the signable URI
    pub(crate) fn uri(&self) -> &Uri {
        &self.uri
    }

    /// Returns the signable HTTP method
    pub(crate) fn method(&self) -> &str {
        self.method
    }

    /// Returns the request headers
    pub(crate) fn headers(&self) -> &[(&str, &str)] {
        self.headers.as_slice()
    }

    /// Returns the signable body
    pub fn body(&self) -> &SignableBody<'_> {
        &self.body
    }
}

/// A signable HTTP request body
#[derive(Debug, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum SignableBody<'a> {
    /// A body composed of a slice of bytes
    Bytes(&'a [u8]),

    /// An unsigned payload
    ///
    /// UnsignedPayload is used for streaming requests where the contents of the body cannot be
    /// known prior to signing
    UnsignedPayload,

    /// A precomputed body checksum. The checksum should be a SHA256 checksum of the body,
    /// lowercase hex encoded. Eg:
    /// `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`
    Precomputed(String),

    /// Set when a streaming body has checksum trailers.
    StreamingUnsignedPayloadTrailer,
}

/// Instructions for applying a signature to an HTTP request.
#[derive(Debug)]
pub struct SigningInstructions {
    headers: Vec<Header>,
    params: Vec<(&'static str, Cow<'static, str>)>,
}

/// Header representation for use in [`SigningInstructions`]
pub struct Header {
    key: &'static str,
    value: String,
    sensitive: bool,
}

impl Debug for Header {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut fmt = f.debug_struct("Header");
        fmt.field("key", &self.key);
        let value = if self.sensitive {
            "** REDACTED **"
        } else {
            &self.value
        };
        fmt.field("value", &value);
        fmt.finish()
    }
}

impl Header {
    /// The name of this header
    pub fn name(&self) -> &'static str {
        self.key
    }

    /// The value of this header
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Whether this header has a sensitive value
    pub fn sensitive(&self) -> bool {
        self.sensitive
    }
}

impl SigningInstructions {
    fn new(headers: Vec<Header>, params: Vec<(&'static str, Cow<'static, str>)>) -> Self {
        Self { headers, params }
    }

    /// Returns the headers and query params that should be applied to this request
    pub fn into_parts(self) -> (Vec<Header>, Vec<(&'static str, Cow<'static, str>)>) {
        (self.headers, self.params)
    }

    /// Returns a reference to the headers that should be added to the request.
    pub fn headers(&self) -> impl Iterator<Item = (&str, &str)> {
        self.headers
            .iter()
            .map(|header| (header.key, header.value.as_str()))
    }

    /// Returns a reference to the query parameters that should be added to the request.
    pub fn params(&self) -> &[(&str, Cow<'static, str>)] {
        self.params.as_slice()
    }

    #[cfg(any(feature = "http0-compat", test))]
    /// Applies the instructions to the given `request`.
    pub fn apply_to_request<B>(self, request: &mut http::Request<B>) {
        let (new_headers, new_query) = self.into_parts();
        for header in new_headers.into_iter() {
            let mut value = http::HeaderValue::from_str(&header.value).unwrap();
            value.set_sensitive(header.sensitive);
            request.headers_mut().insert(header.key, value);
        }

        if !new_query.is_empty() {
            let mut query = aws_smithy_http::query_writer::QueryWriter::new(request.uri());
            for (name, value) in new_query {
                query.insert(name, &value);
            }
            *request.uri_mut() = query.build_uri();
        }
    }
}

/// Produces a signature for the given `request` and returns instructions
/// that can be used to apply that signature to an HTTP request.
pub fn sign<'a>(
    request: SignableRequest<'a>,
    params: &'a SigningParams<'a>,
) -> Result<SigningOutput<SigningInstructions>, SigningError> {
    tracing::trace!(request = ?request, params = ?params, "signing request");
    match params.settings.signature_location {
        SignatureLocation::Headers => {
            let (signing_headers, signature) =
                calculate_signing_headers(&request, params)?.into_parts();
            Ok(SigningOutput::new(
                SigningInstructions::new(signing_headers, vec![]),
                signature,
            ))
        }
        SignatureLocation::QueryParams => {
            let (params, signature) = calculate_signing_params(&request, params)?;
            Ok(SigningOutput::new(
                SigningInstructions::new(vec![], params),
                signature,
            ))
        }
    }
}

type CalculatedParams = Vec<(&'static str, Cow<'static, str>)>;

fn calculate_signing_params<'a>(
    request: &'a SignableRequest<'a>,
    params: &'a SigningParams<'a>,
) -> Result<(CalculatedParams, String), SigningError> {
    let creds = params
        .credentials()
        .ok_or_else(SigningError::unsupported_credential_type)?;
    let creq = CanonicalRequest::from(request, params)?;

    let encoded_creq = &sha256_hex_string(creq.to_string().as_bytes());
    let string_to_sign =
        StringToSign::new(params.time, params.region, params.name, encoded_creq).to_string();
    let signing_key = generate_signing_key(
        creds.secret_access_key(),
        params.time,
        params.region,
        params.name,
    );
    let signature = calculate_signature(signing_key, string_to_sign.as_bytes());
    tracing::trace!(canonical_request = %creq, string_to_sign = %string_to_sign, "calculated signing parameters");

    let values = creq.values.into_query_params().expect("signing with query");
    let mut signing_params = vec![
        (param::X_AMZ_ALGORITHM, Cow::Borrowed(values.algorithm)),
        (param::X_AMZ_CREDENTIAL, Cow::Owned(values.credential)),
        (param::X_AMZ_DATE, Cow::Owned(values.date_time)),
        (param::X_AMZ_EXPIRES, Cow::Owned(values.expires)),
        (
            param::X_AMZ_SIGNED_HEADERS,
            Cow::Owned(values.signed_headers.as_str().into()),
        ),
        (param::X_AMZ_SIGNATURE, Cow::Owned(signature.clone())),
    ];

    if let Some(security_token) = creds.session_token() {
        signing_params.push((
            param::X_AMZ_SECURITY_TOKEN,
            Cow::Owned(security_token.to_string()),
        ));
    }

    Ok((signing_params, signature))
}

/// Calculates the signature headers that need to get added to the given `request`.
///
/// `request` MUST NOT contain any of the following headers:
/// - x-amz-date
/// - x-amz-content-sha-256
/// - x-amz-security-token
fn calculate_signing_headers<'a>(
    request: &'a SignableRequest<'a>,
    params: &'a SigningParams<'a>,
) -> Result<SigningOutput<Vec<Header>>, SigningError> {
    let creds = params
        .credentials()
        .ok_or_else(SigningError::unsupported_credential_type)?;
    // Step 1: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-create-canonical-request.html.
    let creq = CanonicalRequest::from(request, params)?;
    tracing::trace!(canonical_request = %creq);

    // Step 2: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-create-string-to-sign.html.
    let encoded_creq = &sha256_hex_string(creq.to_string().as_bytes());
    let sts = StringToSign::new(params.time, params.region, params.name, encoded_creq);

    // Step 3: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-calculate-signature.html
    let signing_key = generate_signing_key(
        creds.secret_access_key(),
        params.time,
        params.region,
        params.name,
    );
    let signature = calculate_signature(signing_key, sts.to_string().as_bytes());

    // Step 4: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-add-signature-to-request.html
    let values = creq.values.as_headers().expect("signing with headers");
    let mut headers = vec![];
    add_header(&mut headers, header::X_AMZ_DATE, &values.date_time, false);
    headers.push(Header {
        key: "authorization",
        value: build_authorization_header(creds.access_key_id(), &creq, sts, &signature),
        sensitive: false,
    });
    if params.settings.payload_checksum_kind == PayloadChecksumKind::XAmzSha256 {
        add_header(
            &mut headers,
            header::X_AMZ_CONTENT_SHA_256,
            &values.content_sha256,
            false,
        );
    }

    if let Some(security_token) = creds.session_token() {
        add_header(
            &mut headers,
            header::X_AMZ_SECURITY_TOKEN,
            security_token,
            true,
        );
    }

    Ok(SigningOutput::new(headers, signature))
}

fn add_header(map: &mut Vec<Header>, key: &'static str, value: &str, sensitive: bool) {
    map.push(Header {
        key,
        value: value.to_string(),
        sensitive,
    });
}

// add signature to authorization header
// Authorization: algorithm Credential=access key ID/credential scope, SignedHeaders=SignedHeaders, Signature=signature
fn build_authorization_header(
    access_key: &str,
    creq: &CanonicalRequest<'_>,
    sts: StringToSign<'_>,
    signature: &str,
) -> String {
    format!(
        "{} Credential={}/{}, SignedHeaders={}, Signature={}",
        HMAC_256,
        access_key,
        sts.scope,
        creq.values.signed_headers().as_str(),
        signature
    )
}

#[cfg(test)]
mod tests {
    use crate::date_time::test_parsers::parse_date_time;
    use crate::http_request::sign::SignableRequest;
    use crate::http_request::test::{
        test_request, test_signed_request, test_signed_request_query_params,
    };
    use crate::http_request::{
        SessionTokenMode, SignableBody, SignatureLocation, SigningParams, SigningSettings,
    };
    use aws_credential_types::Credentials;
    use http::{HeaderValue, Request};
    use pretty_assertions::assert_eq;
    use proptest::proptest;
    use std::iter;

    use std::time::Duration;

    macro_rules! assert_req_eq {
        (http: $expected:expr, $actual:expr) => {
            let mut expected = ($expected).map(|_b|"body");
            let mut actual = ($actual).map(|_b|"body");
            make_headers_comparable(&mut expected);
            make_headers_comparable(&mut actual);
            assert_eq!(format!("{:?}", expected), format!("{:?}", actual));
        };
        ($expected:tt, $actual:tt) => {
            assert_req_eq!(http: ($expected).as_http_request(), $actual);
        };
    }

    pub(crate) fn make_headers_comparable<B>(request: &mut Request<B>) {
        for (_name, value) in request.headers_mut() {
            value.set_sensitive(false);
        }
    }

    #[test]
    fn test_sign_vanilla_with_headers() {
        let settings = SigningSettings::default();
        let params = SigningParams {
            identity: &Credentials::for_tests().into(),
            region: "us-east-1",
            name: "service",
            time: parse_date_time("20150830T123600Z").unwrap(),
            settings,
        };

        let original = test_request("get-vanilla-query-order-key-case");
        let signable = SignableRequest::from(&original);
        let out = crate::http_request::sign(signable, &params).unwrap();
        assert_eq!(
            "5557820e7380d585310524bd93d51a08d7757fb5efd7344ee12088f2b0860947",
            out.signature
        );

        let mut signed = original.as_http_request();
        out.output.apply_to_request(&mut signed);

        let expected = test_signed_request("get-vanilla-query-order-key-case");
        assert_req_eq!(expected, signed);
    }

    #[test]
    fn test_sign_url_escape() {
        let test = "double-encode-path";
        let settings = SigningSettings::default();
        let params = SigningParams {
            identity: &Credentials::for_tests().into(),
            region: "us-east-1",
            name: "service",
            time: parse_date_time("20150830T123600Z").unwrap(),
            settings,
        };

        let original = test_request(test);
        let signable = SignableRequest::from(&original);
        let out = crate::http_request::sign(signable, &params).unwrap();
        assert_eq!(
            "57d157672191bac40bae387e48bbe14b15303c001fdbb01f4abf295dccb09705",
            out.signature
        );

        let mut signed = original.as_http_request();
        out.output.apply_to_request(&mut signed);

        let expected = test_signed_request(test);
        assert_req_eq!(expected, signed);
    }

    #[test]
    fn test_sign_vanilla_with_query_params() {
        let settings = SigningSettings {
            signature_location: SignatureLocation::QueryParams,
            expires_in: Some(Duration::from_secs(35)),
            ..Default::default()
        };
        let params = SigningParams {
            identity: &Credentials::for_tests().into(),
            region: "us-east-1",
            name: "service",
            time: parse_date_time("20150830T123600Z").unwrap(),
            settings,
        };

        let original = test_request("get-vanilla-query-order-key-case");
        let signable = SignableRequest::from(&original);
        let out = crate::http_request::sign(signable, &params).unwrap();
        assert_eq!(
            "ecce208e4b4f7d7e3a4cc22ced6acc2ad1d170ee8ba87d7165f6fa4b9aff09ab",
            out.signature
        );

        let mut signed = original.as_http_request();
        out.output.apply_to_request(&mut signed);

        let expected = test_signed_request_query_params("get-vanilla-query-order-key-case");
        assert_req_eq!(expected, signed);
    }

    #[test]
    fn test_sign_headers_utf8() {
        let settings = SigningSettings::default();
        let params = SigningParams {
            identity: &Credentials::for_tests().into(),
            region: "us-east-1",
            name: "service",
            time: parse_date_time("20150830T123600Z").unwrap(),
            settings,
        };

        let original = http::Request::builder()
            .uri("https://some-endpoint.some-region.amazonaws.com")
            .header("some-header", HeaderValue::from_str("テスト").unwrap())
            .body("")
            .unwrap()
            .into();
        let signable = SignableRequest::from(&original);
        let out = crate::http_request::sign(signable, &params).unwrap();
        assert_eq!(
            "55e16b31f9bde5fd04f9d3b780dd2b5e5f11a5219001f91a8ca9ec83eaf1618f",
            out.signature
        );

        let mut signed = original.as_http_request();
        out.output.apply_to_request(&mut signed);

        let expected = http::Request::builder()
            .uri("https://some-endpoint.some-region.amazonaws.com")
            .header("some-header", HeaderValue::from_str("テスト").unwrap())
            .header(
                "x-amz-date",
                HeaderValue::from_str("20150830T123600Z").unwrap(),
            )
            .header(
                "authorization",
                HeaderValue::from_str(
                    "AWS4-HMAC-SHA256 \
                        Credential=ANOTREAL/20150830/us-east-1/service/aws4_request, \
                        SignedHeaders=host;some-header;x-amz-date, \
                        Signature=55e16b31f9bde5fd04f9d3b780dd2b5e5f11a5219001f91a8ca9ec83eaf1618f",
                )
                .unwrap(),
            )
            .body("")
            .unwrap();
        assert_req_eq!(http: expected, signed);
    }

    #[test]
    fn test_sign_headers_excluding_session_token() {
        let settings = SigningSettings {
            session_token_mode: SessionTokenMode::Exclude,
            ..Default::default()
        };
        let mut params = SigningParams {
            identity: &Credentials::for_tests().into(),
            region: "us-east-1",
            name: "service",
            time: parse_date_time("20150830T123600Z").unwrap(),
            settings,
        };

        let original = http::Request::builder()
            .uri("https://some-endpoint.some-region.amazonaws.com")
            .body("")
            .unwrap()
            .into();
        let out_without_session_token =
            crate::http_request::sign(SignableRequest::from(&original), &params).unwrap();
        let identity = Credentials::for_tests_with_session_token().into();
        params.identity = &identity;

        let out_with_session_token_but_excluded =
            crate::http_request::sign(SignableRequest::from(&original), &params).unwrap();
        assert_eq!(
            "ab32de057edf094958d178b3c91f3c8d5c296d526b11da991cd5773d09cea560",
            out_with_session_token_but_excluded.signature
        );
        assert_eq!(
            out_with_session_token_but_excluded.signature,
            out_without_session_token.signature
        );

        let mut signed = original.as_http_request();
        out_with_session_token_but_excluded
            .output
            .apply_to_request(&mut signed);

        let expected = http::Request::builder()
            .uri("https://some-endpoint.some-region.amazonaws.com")
            .header(
                "x-amz-date",
                HeaderValue::from_str("20150830T123600Z").unwrap(),
            )
            .header(
                "authorization",
                HeaderValue::from_str(
                    "AWS4-HMAC-SHA256 \
                        Credential=ANOTREAL/20150830/us-east-1/service/aws4_request, \
                        SignedHeaders=host;x-amz-date, \
                        Signature=ab32de057edf094958d178b3c91f3c8d5c296d526b11da991cd5773d09cea560",
                )
                .unwrap(),
            )
            .header(
                "x-amz-security-token",
                HeaderValue::from_str("notarealsessiontoken").unwrap(),
            )
            .body(b"")
            .unwrap();
        assert_req_eq!(http: expected, signed);
    }

    #[test]
    fn test_sign_headers_space_trimming() {
        let settings = SigningSettings::default();
        let params = SigningParams {
            identity: &Credentials::for_tests().into(),
            region: "us-east-1",
            name: "service",
            time: parse_date_time("20150830T123600Z").unwrap(),
            settings,
        };

        let original = http::Request::builder()
            .uri("https://some-endpoint.some-region.amazonaws.com")
            .header(
                "some-header",
                HeaderValue::from_str("  test  test   ").unwrap(),
            )
            .body("")
            .unwrap()
            .into();
        let signable = SignableRequest::from(&original);
        let out = crate::http_request::sign(signable, &params).unwrap();
        assert_eq!(
            "244f2a0db34c97a528f22715fe01b2417b7750c8a95c7fc104a3c48d81d84c08",
            out.signature
        );

        let mut signed = original.as_http_request();
        out.output.apply_to_request(&mut signed);

        let expected = http::Request::builder()
            .uri("https://some-endpoint.some-region.amazonaws.com")
            .header(
                "some-header",
                HeaderValue::from_str("  test  test   ").unwrap(),
            )
            .header(
                "x-amz-date",
                HeaderValue::from_str("20150830T123600Z").unwrap(),
            )
            .header(
                "authorization",
                HeaderValue::from_str(
                    "AWS4-HMAC-SHA256 \
                        Credential=ANOTREAL/20150830/us-east-1/service/aws4_request, \
                        SignedHeaders=host;some-header;x-amz-date, \
                        Signature=244f2a0db34c97a528f22715fe01b2417b7750c8a95c7fc104a3c48d81d84c08",
                )
                .unwrap(),
            )
            .body("")
            .unwrap();
        assert_req_eq!(http: expected, signed);
    }

    proptest! {
        #[test]
        // Only byte values between 32 and 255 (inclusive) are permitted, excluding byte 127, for
        // [HeaderValue](https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.from_bytes).
        fn test_sign_headers_no_panic(
            header in ".*"
        ) {
            let settings = SigningSettings::default();
            let params = SigningParams {
                identity: &Credentials::for_tests().into(),
                region: "us-east-1",
                name: "foo",
                time: std::time::SystemTime::UNIX_EPOCH,
                settings,
            };

            let req = SignableRequest::new(
                "GET",
                "https://foo.com",
                iter::once(("x-sign-me", header.as_str())),
                SignableBody::Bytes(&[])
            );

            if let Ok(req) = req {
                // The test considered a pass if the creation of `creq` does not panic.
                let _creq = crate::http_request::sign(req, &params);
            }
        }
    }
}
