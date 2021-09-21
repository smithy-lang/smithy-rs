/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Utilities to sign HTTP requests.

mod canonical_request;
mod query_writer;
mod settings;
mod sign;
mod url_escape;

#[cfg(test)]
pub(crate) mod test;

pub use settings::{
    PayloadChecksumKind, SignatureLocation, SigningParams, SigningSettings, UriEncoding,
};
pub use sign::{sign, Error, SignableBody, SignableRequest};
