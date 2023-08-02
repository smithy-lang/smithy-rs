/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Functions to create signing keys and calculate signatures.

pub(crate) mod v4;
#[cfg(feature = "sigv4a")]
pub(crate) mod v4a;

pub use v4::{calculate_signature, generate_signing_key};
