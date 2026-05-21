/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Per-partition client and its builder.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use aws_smithy_runtime_api::client::connection::CaptureSmithyConnection;
use aws_smithy_runtime_api::client::connector_metadata::ConnectorMetadata;
use aws_smithy_runtime_api::client::http::{
    HttpClient, HttpConnector, HttpConnectorFuture, HttpConnectorSettings, SharedHttpConnector,
};
use aws_smithy_runtime_api::client::orchestrator::{HttpRequest, HttpResponse};
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;

use super::partition::{DriverSpawner, PartitionId};
use super::{ConnectionPool, SharedPool};
use crate::client::downcast_error;

/// Per-partition view of a [`SharedPool`].
///
/// Implements [`HttpClient`] by routing requests through the shared
/// connection pool. Multiple `Client` instances can reference the same
/// pool, each carrying a distinct [`PartitionId`], network interface
/// binding, and [`DriverSpawner`].
///
/// Construct via [`Client::new`] (default partition) or
/// [`Client::builder`] for per-partition configuration.
///
/// Cloning is cheap: all fields are either `Arc`-backed or small values.
#[derive(Clone)]
pub struct Client {
    pool: SharedPool,
    partition: PartitionId,
    interface: Option<String>,
    driver_spawner: DriverSpawner,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("partition", &self.partition)
            .field("interface", &self.interface)
            .field("driver_spawner", &self.driver_spawner)
            .finish_non_exhaustive()
    }
}

impl Client {
    /// Construct a `Client` using the default partition.
    ///
    /// Equivalent to `Client::builder(pool).build()`.
    pub fn new(pool: &SharedPool) -> Self {
        Self::builder(pool).build()
    }

    /// Return a [`ClientBuilder`] for advanced per-partition configuration.
    pub fn builder(pool: &SharedPool) -> ClientBuilder {
        ClientBuilder {
            pool: pool.clone(),
            partition: PartitionId::default(),
            interface: None,
            driver_spawner: None,
        }
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

/// Builder for configuring a [`Client`] with per-partition settings.
///
/// Three knobs control partition behavior:
///
/// - [`partition`](Self::partition) assigns a [`PartitionId`] (default:
///   anonymous).
/// - [`interface`](Self::interface) binds connections to a network
///   interface (default: none).
/// - [`driver_spawner`](Self::driver_spawner) selects the runtime for
///   connection driver tasks (default: captured from the current tokio
///   runtime at [`build`](Self::build) time).
pub struct ClientBuilder {
    pool: SharedPool,
    partition: PartitionId,
    interface: Option<String>,
    driver_spawner: Option<DriverSpawner>,
}

impl ClientBuilder {
    /// Set the partition identifier (consuming builder).
    pub fn partition(mut self, id: PartitionId) -> Self {
        self.partition = id;
        self
    }

    /// Set the partition identifier (mutable reference).
    pub fn set_partition(&mut self, id: PartitionId) -> &mut Self {
        self.partition = id;
        self
    }

    /// Bind connections to a network interface (consuming builder).
    pub fn interface(mut self, iface: impl Into<String>) -> Self {
        self.interface = Some(iface.into());
        self
    }

    /// Set or clear the network interface binding (mutable reference).
    pub fn set_interface(&mut self, iface: Option<String>) -> &mut Self {
        self.interface = iface;
        self
    }

    /// Set the driver spawner (consuming builder).
    pub fn driver_spawner(mut self, sp: DriverSpawner) -> Self {
        self.driver_spawner = Some(sp);
        self
    }

    /// Set the driver spawner (mutable reference).
    pub fn set_driver_spawner(&mut self, sp: DriverSpawner) -> &mut Self {
        self.driver_spawner = Some(sp);
        self
    }

    /// Build the [`Client`].
    ///
    /// Captures the current tokio runtime handle for the driver spawner
    /// if one was not explicitly provided.
    pub fn build(self) -> Client {
        let driver_spawner = self
            .driver_spawner
            .unwrap_or_else(DriverSpawner::current_tokio);

        Client {
            pool: self.pool,
            partition: self.partition,
            interface: self.interface,
            driver_spawner,
        }
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
    connect_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    sleep_impl: Option<aws_smithy_async::rt::sleep::SharedAsyncSleep>,
    proxy_matcher: Option<Arc<hyper_util::client::proxy::matcher::Matcher>>,
}

impl std::fmt::Debug for PooledConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PooledConnector").finish()
    }
}

impl HttpConnector for PooledConnector {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        let pool = self.pool.clone();
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
                crate::client::proxy::add_proxy_auth_header(&mut request, matcher);
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
                    .zip(sleep_impl)
                    .map(|(d, s)| super::TimeoutContext::new(d, s)),
            );

            let response = pool
                .send_request(connect_ctx, request)
                .await
                .map_err(downcast_error)?;

            HttpResponse::try_from(response).map_err(|err| ConnectorError::other(err.into(), None))
        })
    }
}
