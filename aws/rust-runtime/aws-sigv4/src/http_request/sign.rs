/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::error::SigningError;
use super::{PayloadChecksumKind, SignatureLocation};
use crate::http_request::canonical_request::{header, param, CanonicalRequest, StringToSign};
use crate::http_request::SigningParams;
use crate::sign::v4;
#[cfg(feature = "sigv4a")]
use crate::sign::v4a;
use crate::{SignatureVersion, SigningOutput};
use aws_smithy_http::query_writer::QueryWriter;
use http::{HeaderMap, HeaderValue, Method, Uri};
use std::borrow::Cow;
use std::convert::TryFrom;
use std::str;

/// Represents all of the information necessary to sign an HTTP request.
#[derive(Debug)]
#[non_exhaustive]
pub struct SignableRequest<'a> {
    method: &'a Method,
    uri: &'a Uri,
    headers: &'a HeaderMap<HeaderValue>,
    body: SignableBody<'a>,
}

impl<'a> SignableRequest<'a> {
    /// Creates a new `SignableRequest`. If you have an [`http::Request`], then
    /// consider using [`SignableRequest::from`] instead of `new`.
    pub fn new(
        method: &'a Method,
        uri: &'a Uri,
        headers: &'a HeaderMap<HeaderValue>,
        body: SignableBody<'a>,
    ) -> Self {
        Self {
            method,
            uri,
            headers,
            body,
        }
    }

    /// Returns the signable URI
    pub fn uri(&self) -> &Uri {
        self.uri
    }

    /// Returns the signable HTTP method
    pub fn method(&self) -> &Method {
        self.method
    }

    /// Returns the request headers
    pub fn headers(&self) -> &HeaderMap<HeaderValue> {
        self.headers
    }

    /// Returns the signable body
    pub fn body(&self) -> &SignableBody<'_> {
        &self.body
    }
}

impl<'a, B> From<&'a http::Request<B>> for SignableRequest<'a>
where
    B: 'a,
    B: AsRef<[u8]>,
{
    fn from(request: &'a http::Request<B>) -> SignableRequest<'a> {
        SignableRequest::new(
            request.method(),
            request.uri(),
            request.headers(),
            SignableBody::Bytes(request.body().as_ref()),
        )
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
    headers: Option<HeaderMap<HeaderValue>>,
    params: Option<Vec<(&'static str, Cow<'static, str>)>>,
}

impl SigningInstructions {
    fn new(
        headers: Option<HeaderMap<HeaderValue>>,
        params: Option<Vec<(&'static str, Cow<'static, str>)>>,
    ) -> Self {
        Self { headers, params }
    }

    /// Returns a reference to the headers that should be added to the request.
    pub fn headers(&self) -> Option<&HeaderMap<HeaderValue>> {
        self.headers.as_ref()
    }

    /// Returns the headers and sets the internal value to `None`.
    pub fn take_headers(&mut self) -> Option<HeaderMap<HeaderValue>> {
        self.headers.take()
    }

    /// Returns a reference to the query parameters that should be added to the request.
    pub fn params(&self) -> Option<&Vec<(&'static str, Cow<'static, str>)>> {
        self.params.as_ref()
    }

    /// Returns the query parameters and sets the internal value to `None`.
    pub fn take_params(&mut self) -> Option<Vec<(&'static str, Cow<'static, str>)>> {
        self.params.take()
    }

    /// Applies the instructions to the given `request`.
    pub fn apply_to_request<B>(mut self, request: &mut http::Request<B>) {
        if let Some(new_headers) = self.take_headers() {
            for (name, value) in new_headers.into_iter() {
                request.headers_mut().insert(name.unwrap(), value);
            }
        }
        if let Some(params) = self.take_params() {
            let mut query = QueryWriter::new(request.uri());
            for (name, value) in params {
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
    match params.settings().signature_location {
        SignatureLocation::Headers => {
            let (signing_headers, signature) =
                calculate_signing_headers(&request, params)?.into_parts();
            Ok(SigningOutput::new(
                SigningInstructions::new(Some(signing_headers), None),
                signature,
            ))
        }
        SignatureLocation::QueryParams => {
            let (params, signature) = calculate_signing_params(&request, params)?;
            Ok(SigningOutput::new(
                SigningInstructions::new(None, Some(params)),
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
    let creq = CanonicalRequest::from(request, params)?;
    let encoded_creq = &v4::sha256_hex_string(creq.to_string().as_bytes());
    let creds = params.credentials()?;

    let (signature, string_to_sign) = match params {
        SigningParams::V4(params) => {
            let string_to_sign =
                StringToSign::new_v4(params.time, params.region, params.name, encoded_creq)
                    .to_string();
            let signing_key = v4::generate_signing_key(
                creds.secret_access_key(),
                params.time,
                params.region,
                params.name,
            );
            let signature = v4::calculate_signature(signing_key, string_to_sign.as_bytes());
            (signature, string_to_sign)
        }
        #[cfg(feature = "sigv4a")]
        SigningParams::V4a(params) => {
            let string_to_sign = StringToSign::new_v4a(
                params.time,
                params.region_set,
                params.service_name,
                encoded_creq,
            )
            .to_string();

            // Step 3: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-calculate-signature.html
            let secret_key =
                v4a::generate_signing_key(creds.access_key_id(), creds.secret_access_key());
            let signature = v4a::calculate_signature(&secret_key, string_to_sign.as_bytes());
            (signature, string_to_sign)
        }
    };

    tracing::trace!(
        canonical_request = %creq,
        string_to_sign = %string_to_sign,
        "calculated signing parameters"
    );

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

    #[cfg(feature = "sigv4a")]
    if let Some(region_set) = params.region_set() {
        signing_params.push((param::X_AMZ_REGION_SET, Cow::Owned(region_set.to_owned())));
    }

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
) -> Result<SigningOutput<HeaderMap<HeaderValue>>, SigningError> {
    let creds = params.credentials()?;

    // Step 1: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-create-canonical-request.html.
    let creq = CanonicalRequest::from(request, params)?;
    // Step 2: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-create-string-to-sign.html.
    let encoded_creq = v4::sha256_hex_string(creq.to_string().as_bytes());
    tracing::trace!(canonical_request = %creq);
    let mut headers = HeaderMap::new();

    let signature = match params {
        SigningParams::V4(params) => {
            let sts = StringToSign::new_v4(
                params.time,
                params.region,
                params.name,
                encoded_creq.as_str(),
            );

            // Step 3: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-calculate-signature.html
            let signing_key = v4::generate_signing_key(
                creds.secret_access_key(),
                params.time,
                params.region,
                params.name,
            );
            let signature = v4::calculate_signature(signing_key, sts.to_string().as_bytes());

            // Step 4: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-add-signature-to-request.html
            let values = creq.values.as_headers().expect("signing with headers");
            add_header(&mut headers, header::X_AMZ_DATE, &values.date_time, false);
            headers.insert(
                "authorization",
                build_authorization_header(
                    creds.access_key_id(),
                    &creq,
                    sts,
                    &signature,
                    SignatureVersion::V4,
                ),
            );
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
            signature
        }
        #[cfg(feature = "sigv4a")]
        SigningParams::V4a(params) => {
            let sts = StringToSign::new_v4a(
                params.time,
                params.region_set,
                params.service_name,
                encoded_creq.as_str(),
            );

            // Step 3: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-calculate-signature.html
            let signing_key =
                v4a::generate_signing_key(creds.access_key_id(), creds.secret_access_key());
            let signature = v4a::calculate_signature(&signing_key, sts.to_string().as_bytes());

            // Step 4: https://docs.aws.amazon.com/en_pv/general/latest/gr/sigv4-add-signature-to-request.html
            let values = creq.values.as_headers().expect("signing with headers");
            add_header(&mut headers, header::X_AMZ_DATE, &values.date_time, false);
            add_header(
                &mut headers,
                header::X_AMZ_REGION_SET,
                params.region_set,
                false,
            );
            headers.insert(
                "authorization",
                build_authorization_header(
                    creds.access_key_id(),
                    &creq,
                    sts,
                    &signature,
                    SignatureVersion::V4a,
                ),
            );
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
            signature
        }
    };

    Ok(SigningOutput::new(headers, signature))
}

fn add_header(map: &mut HeaderMap<HeaderValue>, key: &'static str, value: &str, sensitive: bool) {
    let mut value = HeaderValue::try_from(value).expect(key);
    value.set_sensitive(sensitive);
    map.insert(key, value);
}

// add signature to authorization header
// Authorization: algorithm Credential=access key ID/credential scope, SignedHeaders=SignedHeaders, Signature=signature
fn build_authorization_header(
    access_key: &str,
    creq: &CanonicalRequest<'_>,
    sts: StringToSign<'_>,
    signature: &str,
    signature_version: SignatureVersion,
) -> HeaderValue {
    let scope = match signature_version {
        SignatureVersion::V4a => sts.scope.v4a_display(),
        SignatureVersion::V4 => sts.scope.to_string(),
    };
    let mut value = HeaderValue::try_from(format!(
        "{} Credential={}/{}, SignedHeaders={}, Signature={}",
        sts.algorithm,
        access_key,
        scope,
        creq.values.signed_headers().as_str(),
        signature
    ))
    .unwrap();
    value.set_sensitive(true);
    value
}

#[cfg(test)]
mod tests {
    use super::{sign, SigningInstructions};
    use crate::date_time::test_parsers::parse_date_time;
    use crate::http_request::sign::SignableRequest;
    use crate::http_request::test_v4::{
        make_headers_comparable, test_request, test_signed_request,
        test_signed_request_query_params,
    };
    use crate::http_request::{SessionTokenMode, SignatureLocation, SigningSettings};
    use crate::sign::v4;
    use aws_credential_types::Credentials;
    use aws_smithy_runtime_api::client::identity::Identity;
    use http::{HeaderMap, HeaderValue};
    use pretty_assertions::assert_eq;
    use proptest::proptest;
    use std::borrow::Cow;
    use std::time::Duration;

    macro_rules! assert_req_eq {
        ($a:tt, $b:tt) => {
            make_headers_comparable(&mut $a);
            make_headers_comparable(&mut $b);
            assert_eq!(format!("{:#?}", $a), format!("{:#?}", $b))
        };
    }

    #[test]
    fn test_sign_vanilla_with_headers() {
        let settings = SigningSettings::default();
        let params = v4::SigningParams {
            identity: Identity::new(Credentials::for_tests(), None),
            region: "us-east-1",
            name: "service",
            time: parse_date_time("20150830T123600Z").unwrap(),
            settings,
        }
        .into();

        let original = test_request("get-vanilla-query-order-key-case");
        let signable = SignableRequest::from(&original);
        let out = sign(signable, &params).unwrap();
        assert_eq!(
            "5557820e7380d585310524bd93d51a08d7757fb5efd7344ee12088f2b0860947",
            out.signature
        );

        let mut signed = original;
        out.output.apply_to_request(&mut signed);

        let mut expected = test_signed_request("get-vanilla-query-order-key-case");
        assert_req_eq!(expected, signed);
    }

    #[cfg(feature = "sigv4a")]
    mod sigv4a_tests {
        use super::*;
        use crate::http_request::canonical_request::{CanonicalRequest, StringToSign};
        use crate::http_request::{test_v4a, PayloadChecksumKind};
        use crate::sign::{v4, v4a};
        use p256::ecdsa::signature::Verifier;
        use p256::ecdsa::DerSignature;
        use pretty_assertions::assert_eq;
        use std::time::SystemTime;
        use time::format_description::well_known::Rfc3339;
        use time::OffsetDateTime;

        fn new_v4a_signing_params_from_context(
            test_context: &'_ test_v4a::TestContext,
            signature_location: SignatureLocation,
        ) -> crate::http_request::SigningParams<'_> {
            let expires_in = Some(Duration::from_secs(test_context.expiration_in_seconds));

            v4a::SigningParams {
                identity: Identity::new(
                    Credentials::from_keys(
                        &test_context.credentials.access_key_id,
                        &test_context.credentials.secret_access_key,
                        test_context.credentials.token.clone(),
                    ),
                    expires_in.map(|t| SystemTime::UNIX_EPOCH + t),
                ),
                region_set: &test_context.region,
                service_name: &test_context.service,
                time: OffsetDateTime::parse(&test_context.timestamp, &Rfc3339)
                    .unwrap()
                    .into(),
                settings: SigningSettings {
                    // payload_checksum_kind: PayloadChecksumKind::XAmzSha256,
                    expires_in: Some(Duration::from_secs(test_context.expiration_in_seconds)),
                    uri_path_normalization_mode: test_context.normalize.into(),
                    signature_location,
                    session_token_mode: if test_context.omit_session_token {
                        SessionTokenMode::Exclude
                    } else {
                        SessionTokenMode::Include
                    },
                    payload_checksum_kind: if test_context.sign_body {
                        PayloadChecksumKind::XAmzSha256
                    } else {
                        PayloadChecksumKind::NoHeader
                    },
                    ..Default::default()
                },
            }
            .into()
        }

        fn run_v4a_test_suite(test_name: &str, signature_location: SignatureLocation) {
            let tc = test_v4a::test_context(test_name);
            let params = new_v4a_signing_params_from_context(&tc, signature_location);

            let req = test_v4a::test_request(test_name);
            let expected_creq = test_v4a::test_canonical_request(test_name, signature_location);
            let signable_req = SignableRequest::from(&req);
            let actual_creq = CanonicalRequest::from(&signable_req, &params).unwrap();

            assert_eq!(expected_creq, actual_creq.to_string(), "creq didn't match");

            let expected_string_to_sign =
                test_v4a::test_string_to_sign(test_name, signature_location);
            let hashed_creq = &v4::sha256_hex_string(actual_creq.to_string().as_bytes());
            let actual_string_to_sign = StringToSign::new_v4a(
                *params.time(),
                params.region_set().unwrap(),
                params.service_name(),
                hashed_creq,
            )
            .to_string();

            assert_eq!(
                expected_string_to_sign, actual_string_to_sign,
                "'string to sign' didn't match"
            );

            let out = sign(signable_req, &params).unwrap();

            let mut signed = req;
            // Sigv4a signatures are non-deterministic, so we can't compare the signature directly.
            out.output.apply_to_request(&mut signed);

            let creds = params.credentials().unwrap();
            let keypair =
                v4a::generate_signing_key(creds.access_key_id(), creds.secret_access_key());
            let sig = DerSignature::from_bytes(&hex::decode(out.signature).unwrap()).unwrap();

            let peer_public_key = keypair.verifying_key();
            let sts = actual_string_to_sign.as_bytes();
            peer_public_key.verify(sts, &sig).unwrap();
        }

        #[test]
        fn test_get_header_key_duplicate() {
            run_v4a_test_suite("get-header-key-duplicate", SignatureLocation::Headers);
        }

        #[test]
        fn test_get_header_value_order() {
            run_v4a_test_suite("get-header-value-order", SignatureLocation::Headers);
        }

        #[test]
        fn test_get_header_value_trim() {
            run_v4a_test_suite("get-header-value-trim", SignatureLocation::Headers);
        }

        #[test]
        fn test_get_relative_normalized() {
            run_v4a_test_suite("get-relative-normalized", SignatureLocation::Headers);
        }

        #[test]
        fn test_get_relative_relative_normalized() {
            run_v4a_test_suite(
                "get-relative-relative-normalized",
                SignatureLocation::Headers,
            );
        }

        #[test]
        fn test_get_relative_relative_unnormalized() {
            run_v4a_test_suite(
                "get-relative-relative-unnormalized",
                SignatureLocation::Headers,
            );
        }

        #[test]
        fn test_get_relative_unnormalized() {
            run_v4a_test_suite("get-relative-unnormalized", SignatureLocation::Headers);
        }

        #[test]
        fn test_get_slash_dot_slash_normalized() {
            run_v4a_test_suite("get-slash-dot-slash-normalized", SignatureLocation::Headers);
        }

        #[test]
        fn test_get_slash_dot_slash_unnormalized() {
            run_v4a_test_suite(
                "get-slash-dot-slash-unnormalized",
                SignatureLocation::Headers,
            );
        }

        #[test]
        fn test_get_slash_normalized() {
            run_v4a_test_suite("get-slash-normalized", SignatureLocation::Headers);
        }

        #[test]
        fn test_get_slash_pointless_dot_normalized() {
            run_v4a_test_suite(
                "get-slash-pointless-dot-normalized",
                SignatureLocation::Headers,
            );
        }

        #[test]
        fn test_get_slash_pointless_dot_unnormalized() {
            run_v4a_test_suite(
                "get-slash-pointless-dot-unnormalized",
                SignatureLocation::Headers,
            );
        }

        #[test]
        fn test_get_slash_unnormalized() {
            run_v4a_test_suite("get-slash-unnormalized", SignatureLocation::Headers);
        }

        #[test]
        fn test_get_slashes_normalized() {
            run_v4a_test_suite("get-slashes-normalized", SignatureLocation::Headers);
        }

        #[test]
        fn test_get_slashes_unnormalized() {
            run_v4a_test_suite("get-slashes-unnormalized", SignatureLocation::Headers);
        }

        #[test]
        fn test_get_unreserved() {
            run_v4a_test_suite("get-unreserved", SignatureLocation::Headers);
        }

        #[test]
        fn test_get_vanilla() {
            run_v4a_test_suite("get-vanilla", SignatureLocation::Headers);
        }

        #[test]
        fn test_get_vanilla_empty_query_key() {
            run_v4a_test_suite(
                "get-vanilla-empty-query-key",
                SignatureLocation::QueryParams,
            );
        }

        #[test]
        fn test_get_vanilla_query() {
            run_v4a_test_suite("get-vanilla-query", SignatureLocation::QueryParams);
        }

        #[test]
        fn test_get_vanilla_query_order_key_case() {
            run_v4a_test_suite(
                "get-vanilla-query-order-key-case",
                SignatureLocation::QueryParams,
            );
        }

        #[test]
        fn test_get_vanilla_query_unreserved() {
            run_v4a_test_suite(
                "get-vanilla-query-unreserved",
                SignatureLocation::QueryParams,
            );
        }

        #[test]
        fn test_get_vanilla_with_session_token() {
            run_v4a_test_suite("get-vanilla-with-session-token", SignatureLocation::Headers);
        }

        #[test]
        fn test_post_header_key_case() {
            run_v4a_test_suite("post-header-key-case", SignatureLocation::Headers);
        }

        #[test]
        fn test_post_header_key_sort() {
            run_v4a_test_suite("post-header-key-sort", SignatureLocation::Headers);
        }

        #[test]
        fn test_post_header_value_case() {
            run_v4a_test_suite("post-header-value-case", SignatureLocation::Headers);
        }

        #[test]
        fn test_post_sts_header_after() {
            run_v4a_test_suite("post-sts-header-after", SignatureLocation::Headers);
        }

        #[test]
        fn test_post_sts_header_before() {
            run_v4a_test_suite("post-sts-header-before", SignatureLocation::Headers);
        }

        #[test]
        fn test_post_vanilla() {
            run_v4a_test_suite("post-vanilla", SignatureLocation::Headers);
        }

        #[test]
        fn test_post_vanilla_empty_query_value() {
            run_v4a_test_suite(
                "post-vanilla-empty-query-value",
                SignatureLocation::QueryParams,
            );
        }

        #[test]
        fn test_post_vanilla_query() {
            run_v4a_test_suite("post-vanilla-query", SignatureLocation::QueryParams);
        }

        #[test]
        fn test_post_x_www_form_urlencoded() {
            run_v4a_test_suite("post-x-www-form-urlencoded", SignatureLocation::Headers);
        }

        #[test]
        fn test_post_x_www_form_urlencoded_parameters() {
            run_v4a_test_suite(
                "post-x-www-form-urlencoded-parameters",
                SignatureLocation::QueryParams,
            );
        }
    }

    #[test]
    fn test_sign_url_escape() {
        let test = "double-encode-path";
        let settings = SigningSettings::default();
        let params = v4::SigningParams {
            identity: Identity::new(Credentials::for_tests(), None),
            region: "us-east-1",
            name: "service",
            time: parse_date_time("20150830T123600Z").unwrap(),
            settings,
        }
        .into();

        let original = test_request(test);
        let signable = SignableRequest::from(&original);
        let out = sign(signable, &params).unwrap();
        assert_eq!(
            "57d157672191bac40bae387e48bbe14b15303c001fdbb01f4abf295dccb09705",
            out.signature
        );

        let mut signed = original;
        out.output.apply_to_request(&mut signed);

        let mut expected = test_signed_request(test);
        assert_req_eq!(expected, signed);
    }

    #[test]
    fn test_sign_vanilla_with_query_params() {
        let settings = SigningSettings {
            signature_location: SignatureLocation::QueryParams,
            expires_in: Some(Duration::from_secs(35)),
            ..Default::default()
        };
        let params = v4::SigningParams {
            identity: Identity::new(Credentials::for_tests(), None),
            region: "us-east-1",
            name: "service",
            time: parse_date_time("20150830T123600Z").unwrap(),
            settings,
        }
        .into();

        let original = test_request("get-vanilla-query-order-key-case");
        let signable = SignableRequest::from(&original);
        let out = sign(signable, &params).unwrap();
        assert_eq!(
            "ecce208e4b4f7d7e3a4cc22ced6acc2ad1d170ee8ba87d7165f6fa4b9aff09ab",
            out.signature
        );

        let mut signed = original;
        out.output.apply_to_request(&mut signed);

        let mut expected = test_signed_request_query_params("get-vanilla-query-order-key-case");
        assert_req_eq!(expected, signed);
    }

    #[test]
    fn test_sign_headers_utf8() {
        let settings = SigningSettings::default();
        let params = v4::SigningParams {
            identity: Identity::new(Credentials::for_tests(), None),
            region: "us-east-1",
            name: "service",
            time: parse_date_time("20150830T123600Z").unwrap(),
            settings,
        }
        .into();

        let original = http::Request::builder()
            .uri("https://some-endpoint.some-region.amazonaws.com")
            .header("some-header", HeaderValue::from_str("テスト").unwrap())
            .body("")
            .unwrap();
        let signable = SignableRequest::from(&original);
        let out = sign(signable, &params).unwrap();
        assert_eq!(
            "55e16b31f9bde5fd04f9d3b780dd2b5e5f11a5219001f91a8ca9ec83eaf1618f",
            out.signature
        );

        let mut signed = original;
        out.output.apply_to_request(&mut signed);

        let mut expected = http::Request::builder()
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
        assert_req_eq!(expected, signed);
    }

    #[test]
    fn test_sign_headers_excluding_session_token() {
        let settings = SigningSettings {
            session_token_mode: SessionTokenMode::Exclude,
            ..Default::default()
        };
        let params = v4::SigningParams {
            identity: Identity::new(Credentials::for_tests_with_session_token(), None),
            region: "us-east-1",
            name: "service",
            time: parse_date_time("20150830T123600Z").unwrap(),
            settings,
        }
        .into();

        let original = http::Request::builder()
            .uri("https://some-endpoint.some-region.amazonaws.com")
            .body("")
            .unwrap();
        let out_without_session_token = sign(SignableRequest::from(&original), &params).unwrap();

        let out_with_session_token_but_excluded =
            sign(SignableRequest::from(&original), &params).unwrap();
        assert_eq!(
            "ab32de057edf094958d178b3c91f3c8d5c296d526b11da991cd5773d09cea560",
            out_with_session_token_but_excluded.signature
        );
        assert_eq!(
            out_with_session_token_but_excluded.signature,
            out_without_session_token.signature
        );

        let mut signed = original;
        out_with_session_token_but_excluded
            .output
            .apply_to_request(&mut signed);

        let mut expected = http::Request::builder()
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
            .body("")
            .unwrap();
        assert_req_eq!(expected, signed);
    }

    #[test]
    fn test_sign_headers_space_trimming() {
        let settings = SigningSettings::default();
        let params = v4::SigningParams {
            identity: Identity::new(Credentials::for_tests(), None),
            region: "us-east-1",
            name: "service",
            time: parse_date_time("20150830T123600Z").unwrap(),
            settings,
        }
        .into();

        let original = http::Request::builder()
            .uri("https://some-endpoint.some-region.amazonaws.com")
            .header(
                "some-header",
                HeaderValue::from_str("  test  test   ").unwrap(),
            )
            .body("")
            .unwrap();
        let signable = SignableRequest::from(&original);
        let out = sign(signable, &params).unwrap();
        assert_eq!(
            "244f2a0db34c97a528f22715fe01b2417b7750c8a95c7fc104a3c48d81d84c08",
            out.signature
        );

        let mut signed = original;
        out.output.apply_to_request(&mut signed);

        let mut expected = http::Request::builder()
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
        assert_req_eq!(expected, signed);
    }

    #[test]
    fn test_sign_headers_returning_expected_error_on_invalid_utf8() {
        let settings = SigningSettings::default();
        let params = v4::SigningParams {
            identity: Identity::new(Credentials::for_tests(), None),
            region: "us-east-1",
            name: "foo",
            time: std::time::SystemTime::UNIX_EPOCH,
            settings,
        }
        .into();

        let req = http::Request::builder()
            .uri("https://foo.com/")
            .header("x-sign-me", HeaderValue::from_bytes(&[0xC0, 0xC1]).unwrap())
            .body(&[])
            .unwrap();

        let creq = sign(SignableRequest::from(&req), &params);
        assert!(creq.is_err());
    }

    proptest! {
        #[test]
        // Only byte values between 32 and 255 (inclusive) are permitted, excluding byte 127, for
        // [HeaderValue](https://docs.rs/http/latest/http/header/struct.HeaderValue.html#method.from_bytes).
        fn test_sign_headers_no_panic(
            left in proptest::collection::vec(32_u8..=126, 0..100),
            right in proptest::collection::vec(128_u8..=255, 0..100),
        ) {
            let settings = SigningSettings::default();
            let params = v4::SigningParams {
                identity: Identity::new(Credentials::for_tests(), None),
                region: "us-east-1",
                name: "foo",
                time: std::time::SystemTime::UNIX_EPOCH,
                settings,
            }.into();

            let bytes = left.iter().chain(right.iter()).cloned().collect::<Vec<_>>();
            let req = http::Request::builder()
                .uri("https://foo.com/")
                .header("x-sign-me", HeaderValue::from_bytes(&bytes).unwrap())
                .body(&[])
                .unwrap();

            // The test considered a pass if the creation of `creq` does not panic.
            let _creq = crate::http_request::sign(
                SignableRequest::from(&req),
                &params);
        }
    }

    #[test]
    fn apply_signing_instructions_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("some-header", HeaderValue::from_static("foo"));
        headers.insert("some-other-header", HeaderValue::from_static("bar"));
        let instructions = SigningInstructions::new(Some(headers), None);

        let mut request = http::Request::builder()
            .uri("https://some-endpoint.some-region.amazonaws.com")
            .body("")
            .unwrap();

        instructions.apply_to_request(&mut request);

        let get_header = |n: &str| request.headers().get(n).unwrap().to_str().unwrap();
        assert_eq!("foo", get_header("some-header"));
        assert_eq!("bar", get_header("some-other-header"));
    }

    #[test]
    fn apply_signing_instructions_query_params() {
        let params = vec![
            ("some-param", Cow::Borrowed("f&o?o")),
            ("some-other-param?", Cow::Borrowed("bar")),
        ];
        let instructions = SigningInstructions::new(None, Some(params));

        let mut request = http::Request::builder()
            .uri("https://some-endpoint.some-region.amazonaws.com/some/path")
            .body("")
            .unwrap();

        instructions.apply_to_request(&mut request);

        assert_eq!(
            "/some/path?some-param=f%26o%3Fo&some-other-param%3F=bar",
            request.uri().path_and_query().unwrap().to_string()
        );
    }
}
