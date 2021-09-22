/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Provides functions for calculating Sigv4 signing keys, signatures, and
//! optional utilities for signing HTTP requests and Event Stream messages.

// TODO(PresignedReqPrototype): Address lints commented below
// #![warn(
//     missing_debug_implementations,
//     rust_2018_idioms,
//     rustdoc::all,
//     unreachable_pub
// )]

use chrono::{DateTime, Utc};

pub mod sign;

mod date_fmt;

#[cfg(feature = "sign-eventstream")]
pub mod event_stream;

#[cfg(feature = "sign-http")]
pub mod http_request;

/// Parameters to use when signing.
#[non_exhaustive]
#[derive(Debug)]
pub struct SigningParams<'a, S> {
    /// Access Key ID to use.
    pub(crate) access_key: &'a str,
    /// Secret access key to use.
    pub(crate) secret_key: &'a str,
    /// (Optional) Security token to use.
    pub(crate) security_token: Option<&'a str>,

    /// Region to sign for.
    pub(crate) region: &'a str,
    /// AWS Service Name to sign for.
    pub(crate) service_name: &'a str,
    /// Timestamp to use in the signature (should be `Utc::now()` unless testing).
    pub(crate) date_time: DateTime<Utc>,

    /// Additional signing settings. These differ between HTTP and Event Stream.
    pub(crate) settings: S,
}

impl<'a, S: Default> SigningParams<'a, S> {
    pub fn builder() -> Builder<'a, S> {
        Default::default()
    }
}

mod builder {
    use super::SigningParams;
    use chrono::{DateTime, Utc};
    use std::error::Error;
    use std::fmt;

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

    #[derive(Default)]
    pub struct Builder<'a, S> {
        access_key: Option<&'a str>,
        secret_key: Option<&'a str>,
        security_token: Option<&'a str>,
        region: Option<&'a str>,
        service_name: Option<&'a str>,
        date_time: Option<DateTime<Utc>>,
        settings: Option<S>,
    }

    impl<'a, S> Builder<'a, S> {
        pub fn access_key(mut self, access_key: &'a str) -> Self {
            self.access_key = Some(access_key);
            self
        }
        pub fn set_access_key(&mut self, access_key: Option<&'a str>) {
            self.access_key = access_key;
        }

        pub fn secret_key(mut self, secret_key: &'a str) -> Self {
            self.secret_key = Some(secret_key);
            self
        }
        pub fn set_secret_key(&mut self, secret_key: Option<&'a str>) {
            self.secret_key = secret_key;
        }

        pub fn security_token(mut self, security_token: &'a str) -> Self {
            self.security_token = Some(security_token);
            self
        }
        pub fn set_security_token(&mut self, security_token: Option<&'a str>) {
            self.security_token = security_token;
        }

        pub fn region(mut self, region: &'a str) -> Self {
            self.region = Some(region);
            self
        }
        pub fn set_region(&mut self, region: Option<&'a str>) {
            self.region = region;
        }

        pub fn service_name(mut self, service_name: &'a str) -> Self {
            self.service_name = Some(service_name);
            self
        }
        pub fn set_service_name(&mut self, service_name: Option<&'a str>) {
            self.service_name = service_name;
        }

        pub fn date_time(mut self, date_time: DateTime<Utc>) -> Self {
            self.date_time = Some(date_time);
            self
        }
        pub fn set_date_time(&mut self, date_time: Option<DateTime<Utc>>) {
            self.date_time = date_time;
        }

        pub fn settings(mut self, settings: S) -> Self {
            self.settings = Some(settings);
            self
        }
        pub fn set_settings(&mut self, settings: Option<S>) {
            self.settings = settings;
        }

        pub fn build(self) -> Result<SigningParams<'a, S>, BuildError> {
            Ok(SigningParams {
                access_key: self
                    .access_key
                    .ok_or_else(|| BuildError::new("access key is required"))?,
                secret_key: self
                    .secret_key
                    .ok_or_else(|| BuildError::new("secret key is required"))?,
                security_token: self.security_token,
                region: self
                    .region
                    .ok_or_else(|| BuildError::new("region is required"))?,
                service_name: self
                    .service_name
                    .ok_or_else(|| BuildError::new("service name is required"))?,
                date_time: self
                    .date_time
                    .ok_or_else(|| BuildError::new("date time is required"))?,
                settings: self
                    .settings
                    .ok_or_else(|| BuildError::new("settings are required"))?,
            })
        }
    }
}
pub use builder::{BuildError, Builder};

/// Container for the signed output and the signature.
pub struct SigningOutput<T> {
    output: T,
    signature: String,
}

impl<T> SigningOutput<T> {
    pub fn new(output: T, signature: String) -> Self {
        Self { output, signature }
    }

    pub fn output(&self) -> &T {
        &self.output
    }

    pub fn signature(&self) -> &str {
        &self.signature
    }

    pub fn into_parts(self) -> (T, String) {
        (self.output, self.signature)
    }
}
