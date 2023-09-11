/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Test connectors that never return data

use aws_smithy_async::future::never::Never;
use aws_smithy_runtime_api::client::connectors::{HttpConnector, HttpConnectorFuture};
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// A connector that will never respond.
///
/// Returned futures will return Pending forever
#[derive(Clone, Debug, Default)]
pub struct NeverConnector {
    invocations: Arc<AtomicUsize>,
}

impl NeverConnector {
    /// Create a new never connector.
    pub fn new() -> Self {
        Default::default()
    }

    /// Returns the number of invocations made to this connector.
    pub fn num_calls(&self) -> usize {
        self.invocations.load(Ordering::SeqCst)
    }
}

impl HttpConnector for NeverConnector {
    fn call(&self, _request: HttpRequest) -> HttpConnectorFuture {
        self.invocations.fetch_add(1, Ordering::SeqCst);
        HttpConnectorFuture::new(async move {
            Never::new().await;
            unreachable!()
        })
    }
}
