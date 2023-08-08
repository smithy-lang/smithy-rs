/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Utilities to sign HTTP requests.
//!
//! # Example: Signing an HTTP request
//!
//! ```rust
//! # use aws_smithy_runtime_api::client::identity::Identity;
//! fn test() -> Result<(), aws_sigv4::http_request::SigningError> {
//! use aws_sigv4::http_request::{SigningSettings, SigningParams, SignableRequest};
//! use http;
//! use std::time::SystemTime;
//! use aws_credential_types::Credentials;
//!
//! // Create the request to sign
//! let mut request = http::Request::builder()
//!     .uri("https://some-endpoint.some-region.amazonaws.com")
//!     .body("")
//!     .unwrap();
//!
//! // Set up information and settings for the signing
//! let signing_settings = SigningSettings::default();
//! let signing_params = aws_sigv4::sign::v4::SigningParams::builder()
//!     .identity(
//!         Identity::new(
//!             Credentials::new(
//!                 "AKIDEXAMPLE",
//!                 "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
//!                 None,
//!                 None,
//!                 "hardcoded-credentials"
//!             ),
//!             None
//!          )
//!     )
//!     .region("us-east-1")
//!     .name("exampleservice")
//!     .time(SystemTime::now())
//!     .settings(signing_settings)
//!     .build()
//!     .unwrap();
//! // Convert the HTTP request into a signable request
//! let signable_request = SignableRequest::from(&request);
//!
//! // Sign and then apply the signature to the request
//! let (signing_instructions, _signature) = aws_sigv4::http_request::sign(signable_request, &signing_params.into())?.into_parts();
//! signing_instructions.apply_to_request(&mut request);
//! # Ok(())
//! # }
//! ```
//!

pub(crate) mod canonical_request;
mod error;
mod settings;
mod sign;
mod uri_path_normalization;
mod url_escape;

#[cfg(test)]
pub(crate) mod test_v4;

#[cfg(all(test, feature = "sigv4a"))]
pub(crate) mod test_v4a;

use crate::sign::v4;
#[cfg(feature = "sigv4a")]
use crate::sign::v4a;
use crate::SignatureVersion;
use aws_credential_types::Credentials;
pub use error::SigningError;
pub use settings::{
    PayloadChecksumKind, PercentEncodingMode, SessionTokenMode, SignatureLocation, SigningSettings,
    UriPathNormalizationMode,
};
pub use sign::{sign, SignableBody, SignableRequest, SigningInstructions};
use std::time::SystemTime;

// Individual Debug impls are responsible for redacting sensitive fields.
#[derive(Debug)]
#[non_exhaustive]
/// Parameters for signing an HTTP request.
pub enum SigningParams<'a> {
    /// Sign with the SigV4 algorithm
    V4(v4::SigningParams<'a, SigningSettings>),
    #[cfg(feature = "sigv4a")]
    /// Sign with the SigV4a algorithm
    V4a(v4a::SigningParams<'a, SigningSettings>),
}

impl<'a> From<v4::SigningParams<'a, SigningSettings>> for SigningParams<'a> {
    fn from(value: v4::SigningParams<'a, SigningSettings>) -> Self {
        Self::V4(value)
    }
}

#[cfg(feature = "sigv4a")]
impl<'a> From<v4a::SigningParams<'a, SigningSettings>> for SigningParams<'a> {
    fn from(value: v4a::SigningParams<'a, SigningSettings>) -> Self {
        Self::V4a(value)
    }
}

impl<'a> SigningParams<'a> {
    /// Return the credentials within the signing params.
    pub fn credentials(&self) -> Result<&Credentials, error::SigningError> {
        let identity = match self {
            Self::V4(v4::SigningParams { identity, .. }) => identity,
            #[cfg(feature = "sigv4a")]
            Self::V4a(v4a::SigningParams { identity, .. }) => identity,
        };

        identity
            .data::<Credentials>()
            .ok_or_else(error::SigningError::unsupported_identity_type)
    }

    /// If the signing params are for SigV4, return the region. Otherwise, return `None`.
    pub fn region(&self) -> Option<&str> {
        match self {
            SigningParams::V4(v4::SigningParams { region, .. }) => Some(region),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    #[cfg(feature = "sigv4a")]
    /// If the signing params are for SigV4a, return the region set. Otherwise, return `None`.
    pub fn region_set(&self) -> Option<&str> {
        match self {
            SigningParams::V4a(v4a::SigningParams { region_set, .. }) => Some(region_set),
            _ => None,
        }
    }

    #[cfg(test)]
    /// Set the credentials for the signing params.
    pub fn set_credentials(&mut self, credentials: Credentials) {
        use aws_smithy_runtime_api::client::identity::Identity;

        let id = match self {
            Self::V4(v4::SigningParams { identity, .. }) => identity,
            #[cfg(feature = "sigv4a")]
            Self::V4a(v4a::SigningParams { identity, .. }) => identity,
        };

        let expiry = credentials.expiry();
        *id = Identity::new(credentials, expiry);
    }

    /// Return a reference to the settings held by the signing params.
    pub fn settings(&self) -> &SigningSettings {
        match self {
            Self::V4(v4::SigningParams { settings, .. }) => settings,
            #[cfg(feature = "sigv4a")]
            Self::V4a(v4a::SigningParams { settings, .. }) => settings,
        }
    }

    /// Return a mutable reference to the settings held by the signing params.
    pub fn settings_mut(&mut self) -> &mut SigningSettings {
        match self {
            Self::V4(v4::SigningParams { settings, .. }) => settings,
            #[cfg(feature = "sigv4a")]
            Self::V4a(v4a::SigningParams { settings, .. }) => settings,
        }
    }

    #[cfg(test)]
    /// Set the [PayloadChecksumKind] for the signing params.
    pub fn set_payload_checksum_kind(&mut self, kind: PayloadChecksumKind) {
        let settings = self.settings_mut();

        settings.payload_checksum_kind = kind;
    }

    #[cfg(test)]
    /// Set the [SessionTokenMode] for the signing params.
    pub fn set_session_token_mode(&mut self, mode: SessionTokenMode) {
        let settings = self.settings_mut();

        settings.session_token_mode = mode;
    }

    /// Return a reference to the time in the signing params.
    pub fn time(&self) -> &SystemTime {
        match self {
            Self::V4(v4::SigningParams { time, .. }) => time,
            #[cfg(feature = "sigv4a")]
            Self::V4a(v4a::SigningParams { time, .. }) => time,
        }
    }

    /// Return a reference to the service name in the signing params.
    pub fn service_name(&self) -> &str {
        match self {
            Self::V4(v4::SigningParams {
                name: service_name, ..
            }) => service_name,
            #[cfg(feature = "sigv4a")]
            Self::V4a(v4a::SigningParams { service_name, .. }) => service_name,
        }
    }

    /// Return the name of the configured signing algorithm.
    pub fn algorithm(&self) -> &'static str {
        match self {
            Self::V4(params) => params.algorithm(),
            #[cfg(feature = "sigv4a")]
            Self::V4a(params) => params.algorithm(),
        }
    }

    /// Return the name of the signing scheme
    pub fn signature_version(&self) -> SignatureVersion {
        match self {
            Self::V4(..) => SignatureVersion::V4,
            #[cfg(feature = "sigv4a")]
            Self::V4a(..) => SignatureVersion::V4a,
        }
    }
}
