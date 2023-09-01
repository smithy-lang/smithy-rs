/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// Interceptor for connection poisoning.
pub mod connection_poisoning;

#[cfg(feature = "test-util")]
pub mod test_util;

/// Default HTTP and TLS connectors that use hyper and rustls.
#[cfg(feature = "connector-hyper")]
pub mod hyper_connector;

// TODO(enableNewSmithyRuntimeCleanup): Delete this module
/// Unstable API for interfacing the old middleware connectors with the newer orchestrator connectors.
///
/// Important: This module and its contents will be removed in the next release.
pub mod adapter {
    use aws_smithy_client::erase::DynConnector;
    use aws_smithy_runtime_api::client::connectors::{HttpConnector, HttpConnectorFuture};
    use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
    use std::sync::{Arc, Mutex};

    /// Adapts a [`DynConnector`] to the [`HttpConnector`] trait.
    ///
    /// This is a temporary adapter that allows the old-style tower-based connectors to
    /// work with the new non-tower based architecture of the generated clients.
    /// It will be removed in a future release.
    #[derive(Debug)]
    pub struct DynConnectorAdapter {
        // `DynConnector` requires `&mut self`, so we need interior mutability to adapt to it
        dyn_connector: Arc<Mutex<DynConnector>>,
    }

    impl DynConnectorAdapter {
        /// Creates a new `DynConnectorAdapter`.
        pub fn new(dyn_connector: DynConnector) -> Self {
            Self {
                dyn_connector: Arc::new(Mutex::new(dyn_connector)),
            }
        }
    }

    impl HttpConnector for DynConnectorAdapter {
        fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
            let future = self.dyn_connector.lock().unwrap().call_lite(request);
            HttpConnectorFuture::new(future)
        }
    }
}
