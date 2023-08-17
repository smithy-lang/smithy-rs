/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Utilities to sign HTTP requests.
//!
//! # Example: Signing an HTTP request
//!
//! **Note**: This requires `http0-compat` to be enabled.
//!
//! ```rust
//! # use aws_credential_types::Credentials;
//! use aws_smithy_runtime_api::client::identity::Identity;
//! # use aws_sigv4::http_request::SignableBody;
//! #[cfg(feature = "http0-compat")]
//! fn test() -> Result<(), aws_sigv4::http_request::SigningError> {
//! use aws_sigv4::http_request::{sign, SigningSettings, SigningParams, SignableRequest};
//! use http;
//! use std::time::SystemTime;
//!
//! // Set up information and settings for the signing
//! let identity = Credentials::new(
//!     "AKIDEXAMPLE",
//!     "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
//!     None,
//!     None,
//!     "hardcoded-credentials"
//! ).into();
//! let signing_settings = SigningSettings::default();
//! let signing_params = SigningParams::builder()
//!     .identity(&identity)
//!     .region("us-east-1")
//!     .name("exampleservice")
//!     .time(SystemTime::now())
//!     .settings(signing_settings)
//!     .build()
//!     .unwrap();
//! // Convert the HTTP request into a signable request
//! let signable_request = SignableRequest::new(
//!     "GET",
//!     "https://some-endpoint.some-region.amazonaws.com",
//!     std::iter::empty(),
//!     SignableBody::Bytes(&[])
//! ).expect("signable request");
//!
//! let mut my_req = http::Request::new("...");
//! // Sign and then apply the signature to the request
//! let (signing_instructions, _signature) = sign(signable_request, &signing_params)?.into_parts();
//! signing_instructions.apply_to_request(&mut my_req);
//! # Ok(())
//! # }
//! ```
//!

mod canonical_request;
mod error;
mod settings;
mod sign;
mod uri_path_normalization;
mod url_escape;

#[cfg(test)]
pub(crate) mod test;

pub use error::SigningError;
pub use settings::{
    PayloadChecksumKind, PercentEncodingMode, SessionTokenMode, SignatureLocation, SigningParams,
    SigningSettings, UriPathNormalizationMode,
};
pub use sign::{sign, SignableBody, SignableRequest, SigningInstructions};
