/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Shared support for `aws-smithy-http-client` integration tests.

pub(crate) mod client;

#[cfg(any(feature = "__rustls", feature = "s2n-tls"))]
pub(crate) mod tls;
