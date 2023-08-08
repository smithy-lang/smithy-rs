/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sigv4::http_request::{
    PayloadChecksumKind, PercentEncodingMode, SessionTokenMode, SignableBody, SignatureLocation,
    SigningSettings, UriPathNormalizationMode,
};
use aws_smithy_runtime_api::client::auth::AuthSchemeEndpointConfig;
use aws_smithy_runtime_api::client::identity::Identity;
use std::error::Error as StdError;
use std::fmt;
use std::time::Duration;

use aws_smithy_types::config_bag::{Storable, StoreReplace};
use aws_smithy_types::Document;
use aws_smithy_types::Document::Array;
use aws_types::region::{Region, SigningRegion, SigningRegionSet};
use aws_types::SigningName;

/// Auth implementations for SigV4 and SigV4a.
pub mod sigv4;

/// Auth implementations for SigV4a.
pub mod sigv4a;

/// Type of SigV4 signature.
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum HttpSignatureType {
    /// A signature for a full http request should be computed, with header updates applied to the signing result.
    HttpRequestHeaders,

    /// A signature for a full http request should be computed, with query param updates applied to the signing result.
    ///
    /// This is typically used for presigned URLs.
    HttpRequestQueryParams,
}

/// Signing options for SigV4.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct SigningOptions {
    /// Apply URI encoding twice.
    pub double_uri_encode: bool,
    /// Apply a SHA-256 payload checksum.
    pub content_sha256_header: bool,
    /// Normalize the URI path before signing.
    pub normalize_uri_path: bool,
    /// Omit the session token from the signature.
    pub omit_session_token: bool,
    /// Optional override for the payload to be used in signing.
    pub payload_override: Option<SignableBody<'static>>,
    /// Signature type.
    pub signature_type: HttpSignatureType,
    /// Whether or not the signature is optional.
    pub signing_optional: bool,
    /// Optional expiration (for presigning)
    pub expires_in: Option<Duration>,
}

impl Default for SigningOptions {
    fn default() -> Self {
        Self {
            double_uri_encode: true,
            content_sha256_header: false,
            normalize_uri_path: true,
            omit_session_token: false,
            payload_override: None,
            signature_type: HttpSignatureType::HttpRequestHeaders,
            signing_optional: false,
            expires_in: None,
        }
    }
}

/// SigV4 signing configuration for an operation
///
/// Although these fields MAY be customized on a per request basis, they are generally static
/// for a given operation
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SigV4OperationSigningConfig {
    /// AWS Region to sign for.
    pub region: Option<SigningRegion>,
    /// AWS Region to sign for.
    pub region_set: Option<SigningRegionSet>,
    /// AWS Service to sign for.
    pub name: Option<SigningName>,
    /// Signing options.
    pub signing_options: SigningOptions,
}

impl Storable for SigV4OperationSigningConfig {
    type Storer = StoreReplace<Self>;
}

fn settings(operation_config: &SigV4OperationSigningConfig) -> SigningSettings {
    let mut settings = SigningSettings::default();
    settings.percent_encoding_mode = if operation_config.signing_options.double_uri_encode {
        PercentEncodingMode::Double
    } else {
        PercentEncodingMode::Single
    };
    settings.payload_checksum_kind = if operation_config.signing_options.content_sha256_header {
        PayloadChecksumKind::XAmzSha256
    } else {
        PayloadChecksumKind::NoHeader
    };
    settings.uri_path_normalization_mode = if operation_config.signing_options.normalize_uri_path {
        UriPathNormalizationMode::Enabled
    } else {
        UriPathNormalizationMode::Disabled
    };
    settings.session_token_mode = if operation_config.signing_options.omit_session_token {
        SessionTokenMode::Exclude
    } else {
        SessionTokenMode::Include
    };
    settings.signature_location = match operation_config.signing_options.signature_type {
        HttpSignatureType::HttpRequestHeaders => SignatureLocation::Headers,
        HttpSignatureType::HttpRequestQueryParams => SignatureLocation::QueryParams,
    };
    settings.expires_in = operation_config.signing_options.expires_in;
    settings
}

#[derive(Debug)]
enum SigV4SigningError {
    MissingOperationSigningConfig,
    MissingSigningRegion,
    MissingSigningRegionSet,
    MissingSigningName,
    WrongIdentityType(Identity),
    BadTypeInEndpointAuthSchemeConfig(&'static str),
}

impl fmt::Display for SigV4SigningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use SigV4SigningError::*;
        let mut w = |s| f.write_str(s);
        match self {
            MissingOperationSigningConfig => w("missing operation signing config"),
            MissingSigningRegion => w("missing signing region"),
            MissingSigningRegionSet => w("missing signing region set"),
            MissingSigningName => w("missing signing name"),
            WrongIdentityType(identity) => {
                write!(f, "wrong identity type for SigV4/sigV4a. Expected AWS credentials but got `{identity:?}`")
            }
            BadTypeInEndpointAuthSchemeConfig(field_name) => {
                write!(
                    f,
                    "unexpected type for `{field_name}` in endpoint auth scheme config",
                )
            }
        }
    }
}

impl StdError for SigV4SigningError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::MissingOperationSigningConfig => None,
            Self::MissingSigningRegion => None,
            Self::MissingSigningRegionSet => None,
            Self::MissingSigningName => None,
            Self::WrongIdentityType(_) => None,
            Self::BadTypeInEndpointAuthSchemeConfig(_) => None,
        }
    }
}

fn extract_endpoint_auth_scheme_signing_name(
    endpoint_config: &AuthSchemeEndpointConfig<'_>,
) -> Result<Option<SigningName>, SigV4SigningError> {
    use SigV4SigningError::BadTypeInEndpointAuthSchemeConfig as UnexpectedType;

    match extract_field_from_endpoint_config("signingName", endpoint_config) {
        Some(Document::String(s)) => Ok(Some(SigningName::from(s.to_string()))),
        None => Ok(None),
        _ => Err(UnexpectedType("signingName")),
    }
}

fn extract_endpoint_auth_scheme_signing_region(
    endpoint_config: &AuthSchemeEndpointConfig<'_>,
) -> Result<Option<SigningRegion>, SigV4SigningError> {
    use SigV4SigningError::BadTypeInEndpointAuthSchemeConfig as UnexpectedType;

    match extract_field_from_endpoint_config("signingRegion", endpoint_config) {
        Some(Document::String(s)) => Ok(Some(SigningRegion::from(Region::new(s.clone())))),
        None => Ok(None),
        _ => Err(UnexpectedType("signingRegion")),
    }
}

fn extract_endpoint_auth_scheme_signing_region_set(
    endpoint_config: &AuthSchemeEndpointConfig<'_>,
) -> Result<Option<SigningRegionSet>, SigV4SigningError> {
    use SigV4SigningError::BadTypeInEndpointAuthSchemeConfig as UnexpectedType;

    match extract_field_from_endpoint_config("signingRegionSet", endpoint_config) {
        Some(Array(docs)) => {
            // The service defines the region set as a string array. Here, we convert it to a comma separated list.
            let regions: Vec<String> = docs
                .iter()
                .filter_map(|doc| doc.as_string())
                .map(ToString::to_string)
                .collect();
            Ok(Some(SigningRegionSet::from_vec(regions)))
        }
        None => Ok(None),
        _it => Err(UnexpectedType("signingRegionSet")),
    }
}

fn extract_field_from_endpoint_config<'a>(
    field_name: &'static str,
    endpoint_config: &'a AuthSchemeEndpointConfig<'_>,
) -> Option<&'a Document> {
    endpoint_config
        .as_document()
        .and_then(Document::as_object)
        .and_then(|config| config.get(field_name))
}
