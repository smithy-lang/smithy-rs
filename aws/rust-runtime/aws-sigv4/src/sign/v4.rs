/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::date_time::format_date;
use aws_smithy_runtime_api::client::identity::Identity;
use hmac::{digest::FixedOutput, Hmac, Mac};
use sha2::{Digest, Sha256};
use std::time::SystemTime;

/// HashedPayload = Lowercase(HexEncode(Hash(requestPayload)))
#[allow(dead_code)] // Unused when compiling without certain features
pub(crate) fn sha256_hex_string(bytes: impl AsRef<[u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize_fixed())
}

/// Calculates a Sigv4 signature
pub fn calculate_signature(signing_key: impl AsRef<[u8]>, string_to_sign: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(signing_key.as_ref())
        .expect("HMAC can take key of any size");
    mac.update(string_to_sign);
    hex::encode(mac.finalize_fixed())
}

/// Generates a signing key for Sigv4
pub fn generate_signing_key(
    secret: &str,
    time: SystemTime,
    region: &str,
    service: &str,
) -> impl AsRef<[u8]> {
    // kSecret = your secret access key
    // kDate = HMAC("AWS4" + kSecret, Date)
    // kRegion = HMAC(kDate, Region)
    // kService = HMAC(kRegion, Service)
    // kSigning = HMAC(kService, "aws4_request")

    let secret = format!("AWS4{}", secret);
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_ref()).expect("HMAC can take key of any size");
    mac.update(format_date(time).as_bytes());
    let tag = mac.finalize_fixed();

    // sign region
    let mut mac = Hmac::<Sha256>::new_from_slice(&tag).expect("HMAC can take key of any size");
    mac.update(region.as_bytes());
    let tag = mac.finalize_fixed();

    // sign service
    let mut mac = Hmac::<Sha256>::new_from_slice(&tag).expect("HMAC can take key of any size");
    mac.update(service.as_bytes());
    let tag = mac.finalize_fixed();

    // sign request
    let mut mac = Hmac::<Sha256>::new_from_slice(&tag).expect("HMAC can take key of any size");
    mac.update("aws4_request".as_bytes());
    mac.finalize_fixed()
}

/// Parameters to use when signing.
#[derive(Debug)]
#[non_exhaustive]
pub struct SigningParams<'a, S> {
    /// The identity to use when signing a request
    pub(crate) identity: &'a Identity,

    /// Region to sign for.
    pub(crate) region: &'a str,
    /// Service Name to sign for.
    ///
    /// NOTE: Endpoint resolution rules may specify a name that differs from the typical service name.
    pub(crate) name: &'a str,
    /// Timestamp to use in the signature (should be `SystemTime::now()` unless testing).
    pub(crate) time: SystemTime,

    /// Additional signing settings. These differ between HTTP and Event Stream.
    pub(crate) settings: S,
}

const HMAC_256: &str = "AWS4-HMAC-SHA256";

impl<'a, S> SigningParams<'a, S> {
    /// Returns the region that will be used to sign SigV4 requests
    pub fn region(&self) -> &str {
        self.region
    }

    /// Returns the signing name that will be used to sign requests
    pub fn name(&self) -> &str {
        self.name
    }

    /// Return the name of the algorithm used to sign requests
    pub fn algorithm(&self) -> &'static str {
        HMAC_256
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
    use super::SigningParams;
    use aws_smithy_runtime_api::client::identity::Identity;
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
        name: Option<&'a str>,
        time: Option<SystemTime>,
        settings: Option<S>,
    }

    impl<'a, S> Builder<'a, S> {
        /// Sets the identity (required)
        pub fn identity(mut self, identity: &'a Identity) -> Self {
            self.set_identity(Some(identity));
            self
        }
        /// Sets the identity (required)
        pub fn set_identity(&mut self, identity: Option<&'a Identity>) -> &mut Self {
            self.identity = identity;
            self
        }
        ///Sets the region (required)
        pub fn region(mut self, region: &'a str) -> Self {
            self.set_region(Some(region));
            self
        }
        ///Sets the region (required)
        pub fn set_region(&mut self, region: Option<&'a str>) -> &mut Self {
            self.region = region;
            self
        }
        ///Sets the name (required)
        pub fn name(mut self, name: &'a str) -> Self {
            self.set_name(Some(name));
            self
        }
        ///Sets the name (required)
        pub fn set_name(&mut self, name: Option<&'a str>) -> &mut Self {
            self.name = name;
            self
        }
        ///Sets the time to be used in the signature (required)
        pub fn time(mut self, time: SystemTime) -> Self {
            self.set_time(Some(time));
            self
        }
        ///Sets the time to be used in the signature (required)
        pub fn set_time(&mut self, time: Option<SystemTime>) -> &mut Self {
            self.time = time;
            self
        }
        ///Sets additional signing settings (required)
        pub fn settings(mut self, settings: S) -> Self {
            self.set_settings(Some(settings));
            self
        }
        ///Sets additional signing settings (required)
        pub fn set_settings(&mut self, settings: Option<S>) -> &mut Self {
            self.settings = settings;
            self
        }
        /// Builds an instance of [`SigningParams`]. Will yield a [`BuildError`] if
        /// a required argument was not given.
        pub fn build(self) -> Result<SigningParams<'a, S>, BuildError> {
            Ok(SigningParams {
                identity: self
                    .identity
                    .ok_or_else(|| BuildError::new("identity is required"))?,
                region: self
                    .region
                    .ok_or_else(|| BuildError::new("region is required"))?,
                name: self
                    .name
                    .ok_or_else(|| BuildError::new("name is required"))?,
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

#[cfg(test)]
mod tests {
    use super::{calculate_signature, generate_signing_key, sha256_hex_string};
    use crate::date_time::test_parsers::parse_date_time;
    use crate::http_request::test;

    #[test]
    fn test_signature_calculation() {
        let secret = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
        let creq = test::v4::test_canonical_request("iam");
        let time = parse_date_time("20150830T123600Z").unwrap();

        let derived_key = generate_signing_key(secret, time, "us-east-1", "iam");
        let signature = calculate_signature(derived_key, creq.as_bytes());

        let expected = "5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7";
        assert_eq!(expected, &signature);
    }

    #[test]
    fn sign_payload_empty_string() {
        let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let actual = sha256_hex_string([]);
        assert_eq!(expected, actual);
    }
}
