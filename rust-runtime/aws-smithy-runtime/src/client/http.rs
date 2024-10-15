/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// Interceptor for connection poisoning.
pub mod connection_poisoning;

// FIXME - deprecate and re-export from aws-smithy-runtime
#[cfg(feature = "test-util")]
// pub mod test_util;

/// Default HTTP and TLS connectors that use hyper 0.14.x and rustls.
///
/// This module is named after the hyper version number since we anticipate
/// needing to provide equivalent functionality for hyper 1.x in the future.
#[cfg(feature = "connector-hyper-0-14-x")]
#[deprecated = "hyper 0.14.x connector is deprecated, please use the connector-hyper-1-x feature instead"]
pub mod hyper_014 {
    #[allow(deprecated)]
    pub use aws_smithy_http_client::hyper_014::*;
}

/// HTTP body and body-wrapper types
pub mod body;
