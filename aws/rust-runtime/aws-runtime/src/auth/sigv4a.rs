/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_credential_types::Credentials;
use aws_sigv4::http_request::{
    sign, PayloadChecksumKind, PercentEncodingMode, SessionTokenMode, SignableBody,
    SignableRequest, SignatureLocation, SigningParams, SigningSettings, UriPathNormalizationMode,
};
use aws_sigv4::SignatureVersion;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::auth::{
    AuthScheme, AuthSchemeEndpointConfig, AuthSchemeId, Signer,
};
use aws_smithy_runtime_api::client::identity::{Identity, SharedIdentityResolver};
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
use aws_smithy_runtime_api::client::runtime_components::{GetIdentityResolver, RuntimeComponents};
use aws_smithy_types::config_bag::{ConfigBag, Storable, StoreReplace};
use aws_smithy_types::Document;
use aws_smithy_types::Document::Array;
use aws_types::region::SigningRegionSet;
use aws_types::SigningService;
use std::borrow::Cow;
use std::error::Error as StdError;
use std::fmt;
use std::time::{Duration, SystemTime};
use tracing::error;

const EXPIRATION_WARNING: &str = "Presigned request will expire before the given \
        `expires_in` duration because the credentials used to sign it will expire first.";

/// Auth scheme ID for SigV4a.
pub const SCHEME_ID: AuthSchemeId = AuthSchemeId::new("sigv4a");

struct EndpointAuthSchemeConfig {
    signing_region_set_override: Option<SigningRegionSet>,
    signing_service_override: Option<SigningService>,
}

#[derive(Debug)]
enum SigV4aSigningError {
    MissingOperationSigningConfig,
    MissingSigningRegionSet,
    MissingSigningService,
    WrongIdentityType(Identity),
    BadTypeInEndpointAuthSchemeConfig(&'static str),
}

impl fmt::Display for SigV4aSigningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use SigV4aSigningError::*;
        let mut w = |s| f.write_str(s);
        match self {
            MissingOperationSigningConfig => w("missing operation signing config for SigV4a"),
            MissingSigningRegionSet => w("missing signing region for SigV4a signing"),
            MissingSigningService => w("missing signing service for SigV4a signing"),
            WrongIdentityType(identity) => {
                write!(f, "wrong identity type for SigV4a: {identity:?}")
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

impl StdError for SigV4aSigningError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::MissingOperationSigningConfig => None,
            Self::MissingSigningRegionSet => None,
            Self::MissingSigningService => None,
            Self::WrongIdentityType(_) => None,
            Self::BadTypeInEndpointAuthSchemeConfig(_) => None,
        }
    }
}

/// SigV4a auth scheme.
#[derive(Debug, Default)]
pub struct SigV4aAuthScheme {
    signer: SigV4aSigner,
}

impl SigV4aAuthScheme {
    /// Creates a new `SigV4aHttpAuthScheme`.
    pub fn new() -> Self {
        Default::default()
    }
}

impl AuthScheme for SigV4aAuthScheme {
    fn scheme_id(&self) -> AuthSchemeId {
        SCHEME_ID
    }

    fn identity_resolver(
        &self,
        identity_resolvers: &dyn GetIdentityResolver,
    ) -> Option<SharedIdentityResolver> {
        identity_resolvers.identity_resolver(self.scheme_id())
    }

    fn signer(&self) -> &dyn Signer {
        &self.signer
    }
}

/// Type of SigV4a signature.
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum HttpSignatureType {
    /// A signature for a full http request should be computed, with header updates applied to the signing result.
    HttpRequestHeaders,

    /// A signature for a full http request should be computed, with query param updates applied to the signing result.
    ///
    /// This is typically used for presigned URLs.
    HttpRequestQueryParams,
}

/// Signing options for SigV4a.
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

/// SigV4a signing configuration for an operation
///
/// Although these fields MAY be customized on a per request basis, they are generally static
/// for a given operation
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigV4aOperationSigningConfig {
    /// AWS Region to sign for.
    pub region_set: Option<SigningRegionSet>,
    /// AWS Service to sign for.
    pub service: Option<SigningService>,
    /// Signing options.
    pub signing_options: SigningOptions,
}

impl Storable for SigV4aOperationSigningConfig {
    type Storer = StoreReplace<Self>;
}

/// SigV4a HTTP request signer.
#[derive(Debug, Default)]
pub struct SigV4aSigner;

impl SigV4aSigner {
    /// Creates a new signer instance.
    pub fn new() -> Self {
        Self
    }

    fn settings(operation_config: &SigV4aOperationSigningConfig) -> SigningSettings {
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
        settings.uri_path_normalization_mode =
            if operation_config.signing_options.normalize_uri_path {
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

    fn signing_params<'a>(
        settings: SigningSettings,
        credentials: &'a Credentials,
        operation_config: &'a SigV4aOperationSigningConfig,
        request_timestamp: SystemTime,
    ) -> Result<SigningParams<'a>, SigV4aSigningError> {
        if let Some(expires_in) = settings.expires_in {
            if let Some(creds_expires_time) = credentials.expiry() {
                let presigned_expires_time = request_timestamp + expires_in;
                if presigned_expires_time > creds_expires_time {
                    tracing::warn!(EXPIRATION_WARNING);
                }
            }
        }

        let mut builder = SigningParams::builder()
            .signature_version(SignatureVersion::V4a)
            .access_key(credentials.access_key_id())
            .secret_key(credentials.secret_access_key())
            .region(
                operation_config
                    .region_set
                    .as_ref()
                    .ok_or(SigV4aSigningError::MissingSigningRegionSet)?
                    .as_ref(),
            )
            .service_name(
                operation_config
                    .service
                    .as_ref()
                    .ok_or(SigV4aSigningError::MissingSigningService)?
                    .as_ref(),
            )
            .time(request_timestamp)
            .settings(settings);
        builder.set_security_token(credentials.session_token());
        Ok(builder.build().expect("all required fields set"))
    }

    fn extract_operation_config<'a>(
        auth_scheme_endpoint_config: AuthSchemeEndpointConfig<'a>,
        config_bag: &'a ConfigBag,
    ) -> Result<Cow<'a, SigV4aOperationSigningConfig>, SigV4aSigningError> {
        let operation_config = config_bag
            .load::<SigV4aOperationSigningConfig>()
            .ok_or(SigV4aSigningError::MissingOperationSigningConfig)?;

        let signing_region_set = config_bag.load::<SigningRegionSet>();
        let signing_service = config_bag.load::<SigningService>();

        let EndpointAuthSchemeConfig {
            signing_region_set_override,
            signing_service_override,
        } = Self::extract_endpoint_auth_scheme_config(auth_scheme_endpoint_config)?;

        match (
            signing_region_set_override.or_else(|| signing_region_set.cloned()),
            signing_service_override.or_else(|| signing_service.cloned()),
        ) {
            (None, None) => Ok(Cow::Borrowed(operation_config)),
            (region_set, service) => {
                let mut operation_config = operation_config.clone();
                if region_set.is_some() {
                    operation_config.region_set = region_set;
                }
                if service.is_some() {
                    operation_config.service = service;
                }
                Ok(Cow::Owned(operation_config))
            }
        }
    }

    fn extract_endpoint_auth_scheme_config(
        endpoint_config: AuthSchemeEndpointConfig<'_>,
    ) -> Result<EndpointAuthSchemeConfig, SigV4aSigningError> {
        let (mut signing_region_set_override, mut signing_service_override) = (None, None);
        if let Some(config) = endpoint_config.as_document().and_then(Document::as_object) {
            use SigV4aSigningError::BadTypeInEndpointAuthSchemeConfig as UnexpectedType;
            signing_region_set_override = match config.get(SigningRegionSet::type_name()) {
                Some(Array(docs)) => {
                    // The service defines the region set as a string array. Here, we convert it to a comma separated list.
                    let regions: Vec<String> = docs
                        .iter()
                        .filter_map(|doc| doc.as_string())
                        .map(ToString::to_string)
                        .collect();
                    Some(SigningRegionSet::from_vec(regions))
                }
                None => None,
                it => {
                    error!("Unexpected type in endpoint auth scheme config: {:#?}", it);
                    return Err(UnexpectedType(SigningRegionSet::type_name()));
                }
            };
            signing_service_override = match config.get(SigningService::type_name()) {
                Some(Document::String(s)) => Some(SigningService::from(s.to_string())),
                None => None,
                _ => return Err(UnexpectedType(SigningService::type_name())),
            };
        }
        Ok(EndpointAuthSchemeConfig {
            signing_region_set_override,
            signing_service_override,
        })
    }
}

impl Signer for SigV4aSigner {
    fn sign_http_request(
        &self,
        request: &mut HttpRequest,
        identity: &Identity,
        auth_scheme_endpoint_config: AuthSchemeEndpointConfig<'_>,
        runtime_components: &RuntimeComponents,
        config_bag: &ConfigBag,
    ) -> Result<(), BoxError> {
        let operation_config =
            Self::extract_operation_config(auth_scheme_endpoint_config, config_bag)?;
        let request_time = runtime_components.time_source().unwrap_or_default().now();

        let credentials = if let Some(creds) = identity.data::<Credentials>() {
            creds
        } else if operation_config.signing_options.signing_optional {
            tracing::debug!("skipped SigV4a signing since signing is optional for this operation and there are no credentials");
            return Ok(());
        } else {
            return Err(SigV4aSigningError::WrongIdentityType(identity.clone()).into());
        };

        let settings = Self::settings(&operation_config);
        let signing_params =
            Self::signing_params(settings, credentials, &operation_config, request_time)?;

        let (signing_instructions, _signature) = {
            // A body that is already in memory can be signed directly. A body that is not in memory
            // (any sort of streaming body or presigned request) will be signed via UNSIGNED-PAYLOAD.
            let signable_body = operation_config
                .signing_options
                .payload_override
                .as_ref()
                // the payload_override is a cheap clone because it contains either a
                // reference or a short checksum (we're not cloning the entire body)
                .cloned()
                .unwrap_or_else(|| {
                    request
                        .body()
                        .bytes()
                        .map(SignableBody::Bytes)
                        .unwrap_or(SignableBody::UnsignedPayload)
                });

            let signable_request = SignableRequest::new(
                request.method(),
                request.uri(),
                request.headers(),
                signable_body,
            );
            sign(signable_request, &signing_params)?
        }
        .into_parts();

        signing_instructions.apply_to_request(request);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HttpSignatureType, SigV4aOperationSigningConfig, SigV4aSigner, SigningOptions,
        EXPIRATION_WARNING,
    };
    use aws_credential_types::Credentials;
    use aws_sigv4::http_request::SigningSettings;
    use aws_smithy_runtime_api::client::auth::AuthSchemeEndpointConfig;
    use aws_smithy_types::config_bag::{ConfigBag, Layer};
    use aws_smithy_types::Document;
    use aws_types::region::SigningRegionSet;
    use aws_types::SigningService;
    use std::borrow::Cow;
    use std::collections::HashMap;
    use std::time::{Duration, SystemTime};
    use tracing_test::traced_test;

    #[test]
    #[traced_test]
    fn expiration_warning() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        let creds_expire_in = Duration::from_secs(100);

        let mut settings = SigningSettings::default();
        settings.expires_in = Some(creds_expire_in - Duration::from_secs(10));

        let credentials = Credentials::new(
            "test-access-key",
            "test-secret-key",
            Some("test-session-token".into()),
            Some(now + creds_expire_in),
            "test",
        );
        let operation_config = SigV4aOperationSigningConfig {
            region_set: Some(SigningRegionSet::from_static("test")),
            service: Some(SigningService::from_static("test")),
            signing_options: SigningOptions {
                double_uri_encode: true,
                content_sha256_header: true,
                normalize_uri_path: true,
                omit_session_token: true,
                signature_type: HttpSignatureType::HttpRequestHeaders,
                signing_optional: false,
                expires_in: None,
                payload_override: None,
            },
        };
        SigV4aSigner::signing_params(settings, &credentials, &operation_config, now).unwrap();
        assert!(!logs_contain(EXPIRATION_WARNING));

        let mut settings = SigningSettings::default();
        settings.expires_in = Some(creds_expire_in + Duration::from_secs(10));

        SigV4aSigner::signing_params(settings, &credentials, &operation_config, now).unwrap();
        assert!(logs_contain(EXPIRATION_WARNING));
    }

    #[test]
    fn endpoint_config_overrides_region_and_service() {
        let mut layer = Layer::new("test");
        layer.store_put(SigV4aOperationSigningConfig {
            region_set: Some(SigningRegionSet::from_static("override-this-region")),
            service: Some(SigningService::from_static("override-this-service")),
            signing_options: Default::default(),
        });
        let config = Document::Object({
            let mut out = HashMap::new();
            out.insert("name".to_string(), "sigv4a".to_string().into());
            out.insert(
                SigningService::type_name().to_string(),
                "qldb-override".to_string().into(),
            );
            out.insert(
                SigningRegionSet::type_name().to_string(),
                Document::Array(vec!["us-east-override".to_string().into()]),
            );
            out
        });
        let config = AuthSchemeEndpointConfig::from(Some(&config));

        let cfg = ConfigBag::of_layers(vec![layer]);
        let result = SigV4aSigner::extract_operation_config(config, &cfg).expect("success");

        assert_eq!(
            result.region_set,
            Some(SigningRegionSet::from("us-east-override"))
        );
        assert_eq!(
            result.service,
            Some(SigningService::from_static("qldb-override"))
        );
        assert!(matches!(result, Cow::Owned(_)));
    }

    #[test]
    fn endpoint_config_supports_fallback_when_region_or_service_are_unset() {
        let mut layer = Layer::new("test");
        layer.store_put(SigV4aOperationSigningConfig {
            region_set: Some(SigningRegionSet::from_static("us-east-1")),
            service: Some(SigningService::from_static("qldb")),
            signing_options: Default::default(),
        });
        let cfg = ConfigBag::of_layers(vec![layer]);
        let config = AuthSchemeEndpointConfig::empty();

        let result = SigV4aSigner::extract_operation_config(config, &cfg).expect("success");

        assert_eq!(
            result.region_set,
            Some(SigningRegionSet::from_static("us-east-1"))
        );
        assert_eq!(result.service, Some(SigningService::from_static("qldb")));
        assert!(matches!(result, Cow::Borrowed(_)));
    }
}
