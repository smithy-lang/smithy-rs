/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Per-partition client handle.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use aws_smithy_async::rt::sleep::SharedAsyncSleep;
use aws_smithy_runtime_api::client::connection::CaptureSmithyConnection;
use aws_smithy_runtime_api::client::connector_metadata::ConnectorMetadata;
use aws_smithy_runtime_api::client::http::{
    HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings, SharedHttpConnector,
};
use aws_smithy_runtime_api::client::orchestrator::{HttpRequest, HttpResponse};
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use hyper_util::client::proxy::matcher::Matcher as ProxyMatcher;

use super::partition::{PartitionId, PartitionState};
use super::{ConnectionPool, SharedPool};
use crate::client::downcast_error;
use crate::client::proxy::add_proxy_auth_header;

/// Per-partition view of a [`SharedPool`].
///
/// Implements [`HttpClient`] by routing requests through the shared
/// connection pool. Multiple `Client` instances can reference the same
/// pool, each targeting a distinct declared partition.
///
/// Construct via [`Client::new`] (default partition) or
/// [`Client::from_partition`] for a specific declared partition.
///
/// Cloning is cheap: all fields are `Arc`-backed.
#[derive(Clone)]
pub struct Client {
    pool: SharedPool,
    partition: Arc<PartitionState>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("partition_id", &self.partition.id)
            .field("nic", &self.partition.nic)
            .finish_non_exhaustive()
    }
}

impl Client {
    /// Construct a `Client` targeting the pool's default partition (the
    /// first declared, or the anonymous partition when none were declared).
    pub fn new(pool: &SharedPool) -> Self {
        let partition = pool.inner.pool.registry().default_partition();
        Self {
            pool: pool.clone(),
            partition,
        }
    }

    /// Construct a `Client` targeting a specific declared partition.
    /// Panics if `id` was not declared on the pool builder (programming
    /// error: the caller declared the topology).
    pub fn from_partition(pool: &SharedPool, id: PartitionId) -> Self {
        let partition = pool.inner.pool.registry().partition(id);
        Self {
            pool: pool.clone(),
            partition,
        }
    }

    /// The partition id this client targets.
    #[cfg(test)]
    fn partition_id(&self) -> PartitionId {
        self.partition.id
    }
}

impl HttpClient for Client {
    fn http_connector(
        &self,
        settings: &HttpConnectorSettings,
        components: &RuntimeComponents,
    ) -> SharedHttpConnector {
        let connect_timeout = settings.connect_timeout();
        let read_timeout = settings.read_timeout();
        let sleep_impl = components.sleep_impl();

        if (connect_timeout.is_some() || read_timeout.is_some()) && sleep_impl.is_none() {
            panic!(
                "an async sleep impl is required to use connect/read timeouts with \
                 the v2 HTTP client; provide one via `RuntimeComponents::sleep_impl`"
            );
        }

        SharedHttpConnector::new(PooledConnector {
            pool: self.pool.inner.pool.clone(),
            partition: self.partition.clone(),
            connect_timeout,
            read_timeout,
            sleep_impl,
            proxy_matcher: self.pool.inner.proxy_matcher.clone(),
        })
    }

    fn connector_metadata(&self) -> Option<ConnectorMetadata> {
        Some(ConnectorMetadata::new("hyper", Some(Cow::Borrowed("1.x"))))
    }
}

// ---------------------------------------------------------------------------
// PooledConnector (HttpConnector adapter)
// ---------------------------------------------------------------------------

/// Smithy [`HttpConnector`] backed by the v2 connection pool.
///
/// Constructed fresh per [`HttpClient::http_connector`] call so it can
/// capture the per-operation [`HttpConnectorSettings`] (connect/read
/// timeouts). The pool itself is shared across all operations via `Arc`.
struct PooledConnector {
    pool: Arc<ConnectionPool>,
    partition: Arc<PartitionState>,
    connect_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    sleep_impl: Option<SharedAsyncSleep>,
    proxy_matcher: Option<Arc<ProxyMatcher>>,
}

impl std::fmt::Debug for PooledConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PooledConnector").finish()
    }
}

impl HttpConnector for PooledConnector {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        let pool = self.pool.clone();
        let partition = self.partition.clone();
        let connect_timeout = self.connect_timeout;
        let read_timeout = self.read_timeout;
        let sleep_impl = self.sleep_impl.clone();
        let proxy_matcher = self.proxy_matcher.clone();
        HttpConnectorFuture::new(async move {
            let mut request = request
                .try_into_http1x()
                .map_err(|err| ConnectorError::user(err.into()))?;

            let full_uri = request.uri().clone();

            if let Some(matcher) = proxy_matcher.as_ref() {
                add_proxy_auth_header(&mut request, matcher);
            }

            if let Some(capture_smithy) = request.extensions().get::<CaptureSmithyConnection>() {
                let capture = super::ConnectionMetadataCapture::new();
                let for_retriever = capture.clone();
                capture_smithy.set_connection_retriever(move || for_retriever.get());
                request.extensions_mut().insert(capture);
            }

            if let Some((duration, sleep)) = read_timeout.zip(sleep_impl.clone()) {
                request.extensions_mut().insert(super::ReadTimeoutHint(
                    super::TimeoutContext::new(duration, sleep),
                ));
            }

            if !request.headers().contains_key(http_1x::header::HOST) {
                if let Some(authority) = full_uri.authority() {
                    let host = match super::builder::get_non_default_port(&full_uri) {
                        Some(port) => format!("{}:{}", authority.host(), port),
                        None => authority.host().to_string(),
                    };
                    request.headers_mut().insert(
                        http_1x::header::HOST,
                        http_1x::HeaderValue::from_str(&host)
                            .expect("authority is valid header value"),
                    );
                }
            }

            let connect_ctx = super::ConnectCtx::new(
                full_uri,
                connect_timeout
                    .zip(sleep_impl.clone())
                    .map(|(d, s)| super::TimeoutContext::new(d, s)),
            )
            .with_sleep(sleep_impl);

            let response = pool
                .send_request(&partition, connect_ctx, request)
                .await
                .map_err(downcast_error)?;

            HttpResponse::try_from(response).map_err(|err| ConnectorError::other(err.into(), None))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::pool::partition::{Partition, TokioDriverSpawner};

    #[tokio::test]
    async fn client_new_uses_default_partition() {
        let pool = SharedPool::builder().build_http();
        let client = Client::new(&pool);
        assert_eq!(client.partition_id(), PartitionId::default());
    }

    #[tokio::test]
    async fn from_partition_resolves_declared() {
        let pool = SharedPool::builder()
            .partitions([Partition::new(
                PartitionId::from_index(3),
                TokioDriverSpawner::current(),
            )])
            .build_http();
        let client = Client::from_partition(&pool, PartitionId::from_index(3));
        assert_eq!(client.partition_id(), PartitionId::from_index(3));
    }

    #[tokio::test]
    #[should_panic(expected = "partition not declared")]
    async fn from_partition_unknown_panics() {
        let pool = SharedPool::builder().build_http();
        Client::from_partition(&pool, PartitionId::from_index(99));
    }
}
