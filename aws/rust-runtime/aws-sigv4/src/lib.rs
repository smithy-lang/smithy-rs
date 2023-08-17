/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Provides functions for calculating Sigv4 signing keys, signatures, and
//! optional utilities for signing HTTP requests and Event Stream messages.

#![allow(clippy::derive_partial_eq_without_eq)]
#![warn(
    missing_docs,
    rustdoc::missing_crate_level_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

use aws_credential_types::Credentials;
use aws_smithy_runtime_api::client::identity::Identity;
use std::time::SystemTime;

pub mod sign;

mod date_time;

#[cfg(feature = "sign-eventstream")]
pub mod event_stream;

#[cfg(feature = "sign-http")]
pub mod http_request;

/// Parameters to use when signing.
// #[derive(Debug)] is safe because `Identity` handles redaction.
#[derive(Debug)]
#[non_exhaustive]
pub struct SigningParams<'a, S> {
    pub(crate) identity: &'a Identity,

    /// Region to sign for.
    pub(crate) region: &'a str,
    /// AWS Service Name to sign for.
    pub(crate) service_name: &'a str,
    /// Timestamp to use in the signature (should be `SystemTime::now()` unless testing).
    pub(crate) time: SystemTime,

    /// Additional signing settings. These differ between HTTP and Event Stream.
    pub(crate) settings: S,
}

impl<'a, S> SigningParams<'a, S> {
    /// Returns the region that will be used to sign
    pub fn region(&self) -> &str {
        self.region
    }

    /// Returns the service name that will be used to sign
    pub fn service_name(&self) -> &str {
        self.service_name
    }

    /// If the identity in params contains AWS credentials, return them. Otherwise, return `None`.
    pub(crate) fn credentials(&self) -> Option<&Credentials> {
        self.identity.data::<Credentials>()
    }
}

impl<'a, S: Default> SigningParams<'a, S> {
    /// Returns a builder that can create new `SigningParams`.
    pub fn builder() -> signing_params::Builder<'a, S> {
        Default::default()
    }
}

/// Builder and error for creating [`SigningParams`]
pub mod signing_params {
    use super::{Identity, SigningParams};
    use std::error::Error;
    use std::fmt;
    use std::time::SystemTime;

    /// [`SigningParams`] builder error
    #[derive(Debug)]
    pub struct BuildError {
        reason: &'static str,
    }
    impl BuildError {
        fn new(reason: &'static str) -> Self {
            Self { reason }
        }
    }

    impl fmt::Display for BuildError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.reason)
        }
    }

    impl Error for BuildError {}

    /// Builder that can create new [`SigningParams`]
    #[derive(Debug, Default)]
    pub struct Builder<'a, S> {
        identity: Option<&'a Identity>,
        region: Option<&'a str>,
        service_name: Option<&'a str>,
        time: Option<SystemTime>,
        settings: Option<S>,
    }

    impl<'a, S> Builder<'a, S> {
        /// Sets the identity (required).
        pub fn identity(mut self, identity: &'a Identity) -> Self {
            self.identity = Some(identity);
            self
        }
        /// Sets the identity (required)
        pub fn set_identity(&mut self, identity: Option<&'a Identity>) {
            self.identity = identity;
        }

        /// Sets the region (required)
        pub fn region(mut self, region: &'a str) -> Self {
            self.region = Some(region);
            self
        }
        /// Sets the region (required)
        pub fn set_region(&mut self, region: Option<&'a str>) {
            self.region = region;
        }

        /// Sets the service name (required)
        pub fn service_name(mut self, service_name: &'a str) -> Self {
            self.service_name = Some(service_name);
            self
        }
        /// Sets the service name (required)
        pub fn set_service_name(&mut self, service_name: Option<&'a str>) {
            self.service_name = service_name;
        }

        /// Sets the time to be used in the signature (required)
        pub fn time(mut self, time: SystemTime) -> Self {
            self.time = Some(time);
            self
        }
        /// Sets the time to be used in the signature (required)
        pub fn set_time(&mut self, time: Option<SystemTime>) {
            self.time = time;
        }

        /// Sets additional signing settings (required)
        pub fn settings(mut self, settings: S) -> Self {
            self.settings = Some(settings);
            self
        }
        /// Sets additional signing settings (required)
        pub fn set_settings(&mut self, settings: Option<S>) {
            self.settings = settings;
        }

        /// Builds an instance of [`SigningParams`]. Will yield a [`BuildError`] if
        /// a required argument was not given.
        pub fn build(self) -> Result<SigningParams<'a, S>, BuildError> {
            Ok(SigningParams {
                identity: self
                    .identity
                    .ok_or_else(|| BuildError::new("an identity is required"))?,
                region: self
                    .region
                    .ok_or_else(|| BuildError::new("region is required"))?,
                service_name: self
                    .service_name
                    .ok_or_else(|| BuildError::new("service name is required"))?,
                time: self
                    .time
                    .ok_or_else(|| BuildError::new("time is required"))?,
                settings: self
                    .settings
                    .ok_or_else(|| BuildError::new("settings are required"))?,
            })
        }
    }
}

/// Container for the signed output and the signature.
///
/// This is returned by signing functions, and the signed output will be
/// different based on what is being signed (for example, an event stream
/// message, or an HTTP request).
#[derive(Debug)]
pub struct SigningOutput<T> {
    output: T,
    signature: String,
}

impl<T> SigningOutput<T> {
    /// Creates a new [`SigningOutput`]
    pub fn new(output: T, signature: String) -> Self {
        Self { output, signature }
    }

    /// Returns the signed output
    pub fn output(&self) -> &T {
        &self.output
    }

    /// Returns the signature as a lowercase hex string
    pub fn signature(&self) -> &str {
        &self.signature
    }

    /// Decomposes the `SigningOutput` into a tuple of the signed output and the signature
    pub fn into_parts(self) -> (T, String) {
        (self.output, self.signature)
    }
}
