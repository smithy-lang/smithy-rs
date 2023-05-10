/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Functionality related to creating new HTTP Connectors

use aws_smithy_client::erase::DynConnector;

// unused when all crate features are disabled
/// Unwrap an [`Option<DynConnector>`](aws_smithy_client::erase::DynConnector), and panic with a helpful error message if it's `None`
pub(crate) fn expect_connector(connector: Option<DynConnector>) -> DynConnector {
    connector.expect("No HTTP connector was available. Enable the `rustls` or `native-tls` crate feature or set a connector to fix this.")
}

pub use aws_smithy_client::conns::default_connector;
