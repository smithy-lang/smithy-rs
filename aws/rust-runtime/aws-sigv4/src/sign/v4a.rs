/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::client::identity::Identity;
use bytes::{BufMut, BytesMut};
use num_bigint::BigInt;
use once_cell::sync::Lazy;
use p256::ecdsa::{signature::Signer, DerSignature, SigningKey};
use std::fmt;
use std::io::Write;
use std::time::SystemTime;
use zeroize::Zeroizing;

const ALGORITHM: &[u8] = b"AWS4-ECDSA-P256-SHA256";
static BIG_N_MINUS_2: Lazy<BigInt> = Lazy::new(|| {
    const ORDER: &[u32] = &[
        0xFFFFFFFF, 0x00000000, 0xFFFFFFFF, 0xFFFFFFFF, 0xBCE6FAAD, 0xA7179E84, 0xF3B9CAC2,
        0xFC632551,
    ];
    let big_n = BigInt::from_slice(num_bigint::Sign::Plus, ORDER);
    big_n - BigInt::from(2i32)
});

/// Calculates a Sigv4a signature
pub fn calculate_signature(signing_key: &SigningKey, string_to_sign: &[u8]) -> String {
    let signature: DerSignature = signing_key.sign(string_to_sign);
    hex::encode(signature.as_ref())
}

/// Generates a signing key for Sigv4a.
pub fn generate_signing_key(access_key: &str, secret_access_key: &str) -> SigningKey {
    // Capacity is the secret access key length plus the length of "AWS4A"
    let mut input_key = Zeroizing::new(Vec::with_capacity(secret_access_key.len() + 5));
    write!(input_key, "AWS4A{secret_access_key}").unwrap();

    // Capacity is the access key length plus the counter byte
    let mut kdf_context = Zeroizing::new(Vec::with_capacity(access_key.len() + 1));
    let mut counter = Zeroizing::new(1u8);
    let key = loop {
        write!(kdf_context, "{access_key}").unwrap();
        kdf_context.push(*counter);

        let mut fis = ALGORITHM.to_vec();
        fis.push(0);
        fis.append(&mut kdf_context);
        fis.put_i32(256);

        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &input_key);

        let mut buf = BytesMut::new();
        buf.put_i32(1);
        buf.put_slice(&fis);
        let tag = ring::hmac::sign(&key, &buf);
        let tag = &tag.as_ref()[0..32];

        let k0 = BigInt::from_bytes_be(num_bigint::Sign::Plus, tag);

        // It would be more secure for this to be a constant time comparison, but because this
        // is for client usage, that's not as big a deal.
        if k0 <= *BIG_N_MINUS_2 {
            let pk = k0 + BigInt::from(1i32);
            let d = Zeroizing::new(pk.to_bytes_be().1);
            break SigningKey::from_slice(&d).unwrap();
        }

        *counter = counter
            .checked_add(1)
            .expect("counter will never get to 255");
    };

    key
}

/// Parameters to use when signing.
#[non_exhaustive]
pub struct SigningParams<'a, S> {
    /// The identity to use when signing a request
    pub(crate) identity: Identity,
    /// Region set to sign for.
    pub(crate) region_set: &'a str,
    /// AWS Service Name to sign for.
    pub(crate) service_name: &'a str,
    /// Timestamp to use in the signature (should be `SystemTime::now()` unless testing).
    pub(crate) time: SystemTime,

    /// Additional signing settings. These differ between HTTP and Event Stream.
    pub(crate) settings: S,
}

pub(crate) const ECDSA_256: &str = "AWS4-ECDSA-P256-SHA256";

impl<'a, S> SigningParams<'a, S> {
    /// Returns the region that will be used to sign SigV4a requests
    pub fn region_set(&self) -> &str {
        self.region_set
    }

    /// Returns the service name that will be used to sign requests
    pub fn service_name(&self) -> &str {
        self.service_name
    }

    /// Return the name of the algorithm used to sign requests
    pub fn algorithm(&self) -> &'static str {
        ECDSA_256
    }
}

impl<'a, S: fmt::Debug> fmt::Debug for SigningParams<'a, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigningParams")
            .field("identity", &"** redacted **")
            .field("region_set", &self.region_set)
            .field("service_name", &self.service_name)
            .field("time", &self.time)
            .field("settings", &self.settings)
            .finish()
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
    use aws_smithy_runtime_api::builder_methods;
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
        identity: Option<Identity>,
        region_set: Option<&'a str>,
        service_name: Option<&'a str>,
        time: Option<SystemTime>,
        settings: Option<S>,
    }

    impl<'a, S> Builder<'a, S> {
        builder_methods!(
            set_identity,
            identity,
            Identity,
            "Sets the identity (required)",
            set_region_set,
            region_set,
            &'a str,
            "Sets the region set (required)",
            set_service_name,
            service_name,
            &'a str,
            "Sets the service_name (required)",
            set_time,
            time,
            SystemTime,
            "Sets the time to be used in the signature (required)",
            set_settings,
            settings,
            S,
            "Sets additional signing settings (required)"
        );

        /// Builds an instance of [`SigningParams`]. Will yield a [`BuildError`] if
        /// a required argument was not given.
        pub fn build(self) -> Result<SigningParams<'a, S>, BuildError> {
            Ok(SigningParams {
                identity: self
                    .identity
                    .ok_or_else(|| BuildError::new("An identity is required"))?,
                region_set: self
                    .region_set
                    .ok_or_else(|| BuildError::new("region_set is required"))?,
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
