/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Local HTTP/1 acquisition, dispatch, and response completion.

use super::admission::ProtocolRequirement;
use super::cell::h1::{H1ReturnTask, H1Selection};
use super::cell::{AcquisitionEvent, AcquisitionResult, OriginCell};
use super::connection::{CloseReason, ConnectionState, DispatchGuard};
use super::handshake::{self, ConnectTimeout};
use super::partition::DriverSpawner;
use super::registry::PartitionState;
use super::ConnectionPool;
use crate::sync::Arc;
use aws_smithy_runtime_api::client::connection::CaptureSmithyConnection;
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_types::body::SdkBody;
use http_1x::{Method, Request, Response, Uri, Version};
use hyper::body::Body;
use std::future::poll_fn;
use std::pin::Pin;
use std::sync::Arc as StdArc;
use std::task::{Context, Poll};

impl ConnectionPool {
    /// Sends one request through the resolved partition's local H1 state.
    pub(super) async fn send_request(
        &self,
        partition: Arc<PartitionState>,
        mut request: Request<SdkBody>,
        connect_timeout: Option<ConnectTimeout>,
    ) -> Result<Response<SdkBody>, ConnectorError> {
        let absolute_uri = request.uri().clone();
        let cell = self
            .inner
            .registry
            .resolve_cell(&partition, &absolute_uri)
            .map_err(|error| ConnectorError::user(error.into()))?;
        tracing::trace!(
            request_partition = ?partition.id(),
            origin_scheme = %cell.id().origin().scheme(),
            origin_host = cell.id().origin().host(),
            origin_port = ?cell.id().origin().port(),
            "routed request to connection-pool cell"
        );
        let owner_spawner = handshake::owner_spawner(&partition)
            .map_err(|error| ConnectorError::user(error.into()))?;
        partition.start_maintenance(owner_spawner.as_ref());

        validate_request_before_acquisition(&request)
            .map_err(|error| ConnectorError::user(error.into()))?;
        add_host_header(&mut request, &absolute_uri)
            .map_err(|error| ConnectorError::user(error.into()))?;

        loop {
            let mut selection = self
                .acquire_h1(
                    partition.clone(),
                    cell.clone(),
                    absolute_uri.clone(),
                    connect_timeout.clone(),
                    owner_spawner.clone(),
                )
                .await?;
            let request_method = request.method().clone();
            let reused = selection.is_reused();
            let connection = selection.connection().clone();
            let close_handle = selection.close_handle();
            let captured_metadata = request
                .extensions()
                .get::<CaptureSmithyConnection>()
                .cloned()
                .map(|capture| {
                    let metadata = connection.info().metadata(close_handle.clone());
                    let captured = metadata.clone();
                    capture.set_connection_retriever(move || Some(captured.clone()));
                    metadata
                });

            if request.version() == Version::HTTP_2 {
                let metadata =
                    captured_metadata.unwrap_or_else(|| connection.info().metadata(close_handle));
                return Err(ConnectorError::user(
                    "an HTTP/2 request cannot use an HTTP/1 connection".into(),
                )
                .with_connection(metadata));
            }

            rewrite_h1_request_target(&mut request, connection.info().is_proxied());
            let Some(dispatch) = ConnectionState::try_commit_dispatch(&connection) else {
                tracing::trace!(
                    connection_id = %connection.id(),
                    "HTTP/1 selection became stale before dispatch; retrying"
                );
                *request.uri_mut() = absolute_uri.clone();
                selection.retire(CloseReason::ProtocolClosed);
                continue;
            };
            let send = selection.sender_mut().hyper_mut().try_send_request(request);

            let return_task = selection.into_return_task();
            let response = send.await;
            match response {
                Ok(mut response) => {
                    connection
                        .info()
                        .apply_connector_extras(response.extensions_mut());
                    return Ok(guard_response(
                        response,
                        request_method,
                        return_task,
                        dispatch,
                        owner_spawner.clone(),
                    ));
                }
                Err(mut error) => {
                    if let Some(mut returned) = error.take_message() {
                        return_task.retire(CloseReason::ProtocolClosed);
                        drop(dispatch);
                        if reused {
                            tracing::trace!(
                                connection_id = %connection.id(),
                                "reused HTTP/1 connection rejected request; retrying"
                            );
                            *returned.uri_mut() = absolute_uri.clone();
                            request = returned;
                            continue;
                        }
                        let metadata = captured_metadata
                            .unwrap_or_else(|| connection.info().metadata(close_handle));
                        return Err(super::super::downcast_error(Box::new(error.into_error()))
                            .with_connection(metadata));
                    }
                    return_task.retire(CloseReason::IncompleteH1Exchange);
                    drop(dispatch);
                    let metadata = captured_metadata
                        .unwrap_or_else(|| connection.info().metadata(close_handle));
                    return Err(super::super::downcast_error(Box::new(error.into_error()))
                        .with_connection(metadata));
                }
            }
        }
    }

    /// Acquires a local H1 selection or starts one owner-partition attempt.
    async fn acquire_h1(
        &self,
        partition: Arc<PartitionState>,
        cell: Arc<OriginCell>,
        uri: Uri,
        connect_timeout: Option<ConnectTimeout>,
        owner_spawner: StdArc<dyn DriverSpawner>,
    ) -> Result<H1Selection, ConnectorError> {
        if let Some(selection) = OriginCell::select_h1(&cell) {
            tracing::trace!(
                connection_id = %selection.connection_id(),
                request_partition = ?partition.id(),
                "HTTP/1 pool hit"
            );
            return Ok(selection);
        }

        tracing::trace!(
            request_partition = ?partition.id(),
            origin_scheme = %cell.id().origin().scheme(),
            origin_host = cell.id().origin().host(),
            origin_port = ?cell.id().origin().port(),
            "HTTP/1 pool miss"
        );
        let waiter = cell.register_waiter(ProtocolRequirement::H1Compatible);
        let mut waiter_guard = WaiterGuard::new(cell.clone(), waiter);
        loop {
            match poll_fn(|cx| cell.poll_waiter(waiter, cx)).await {
                AcquisitionEvent::Complete(AcquisitionResult::H1(selection)) => {
                    tracing::trace!(
                        connection_id = %selection.connection_id(),
                        request_partition = ?partition.id(),
                        "HTTP/1 waiter received a reusable connection"
                    );
                    waiter_guard.disarm();
                    return Ok(selection);
                }
                AcquisitionEvent::Complete(AcquisitionResult::Failed(error)) => {
                    waiter_guard.disarm();
                    return Err(error);
                }
                AcquisitionEvent::Establish(permit) => {
                    tracing::trace!(
                        request_partition = ?partition.id(),
                        "HTTP/1 waiter received establishment capacity"
                    );
                    let attempt = handshake::establish_h1(
                        self.inner.clone(),
                        partition.clone(),
                        cell.clone(),
                        uri.clone(),
                        owner_spawner.clone(),
                        permit,
                        connect_timeout.clone(),
                    );
                    let completion = EstablishmentCompletion::new(cell.clone(), waiter);
                    owner_spawner.spawn(Box::pin(async move {
                        let mut completion = completion;
                        if !completion.start() {
                            drop(attempt);
                            completion.disarm();
                            return;
                        }
                        let result = attempt
                            .await
                            .map(AcquisitionResult::H1)
                            .unwrap_or_else(AcquisitionResult::Failed);
                        completion.complete(result);
                    }));
                }
            }
        }
    }
}

/// Completes one establishment episode even if its owner task is dropped.
///
/// The guard is captured by the submitted future before its first poll. A
/// runtime that discards that future therefore produces a terminal waiter
/// result instead of leaving `Launching` parked indefinitely.
struct EstablishmentCompletion {
    /// Cell that owns the launching waiter.
    cell: Arc<OriginCell>,
    /// Waiter whose establishment episode must become terminal.
    waiter: super::cell::WaiterId,
    /// Whether `Drop` still owes terminal failure delivery.
    active: bool,
}

impl EstablishmentCompletion {
    /// Arms completion for one waiter before the future is submitted.
    fn new(cell: Arc<OriginCell>, waiter: super::cell::WaiterId) -> Self {
        Self {
            cell,
            waiter,
            active: true,
        }
    }

    /// Claims establishment immediately before the connector is first polled.
    fn start(&self) -> bool {
        self.cell.start_establishment(self.waiter)
    }

    /// Delivers the terminal attempt result and disarms task-drop fallback.
    fn complete(mut self, result: AcquisitionResult) {
        self.active = false;
        self.cell.complete_establishment(self.waiter, result);
    }

    /// Disarms fallback when another result already completed the waiter.
    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for EstablishmentCompletion {
    fn drop(&mut self) {
        if self.active {
            self.cell.complete_establishment(
                self.waiter,
                AcquisitionResult::Failed(ConnectorError::other(
                    EstablishmentTaskDropped.into(),
                    None,
                )),
            );
        }
    }
}

#[derive(Debug)]
struct EstablishmentTaskDropped;

impl std::fmt::Display for EstablishmentTaskDropped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the owner-runtime connection establishment task was dropped")
    }
}

impl std::error::Error for EstablishmentTaskDropped {}

/// Cancels a retained acquisition episode when its request future is dropped.
struct WaiterGuard {
    /// Cell retaining the acquisition episode.
    cell: Arc<OriginCell>,
    /// Episode cancelled if the request future does not consume a result.
    waiter: super::cell::WaiterId,
    /// Whether `Drop` still owes waiter cancellation.
    active: bool,
}

impl WaiterGuard {
    /// Arms cancellation for one retained waiter.
    fn new(cell: Arc<OriginCell>, waiter: super::cell::WaiterId) -> Self {
        Self {
            cell,
            waiter,
            active: true,
        }
    }

    /// Transfers terminal ownership to the requesting task.
    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for WaiterGuard {
    fn drop(&mut self) {
        if self.active {
            OriginCell::cancel_waiter(&self.cell, self.waiter);
        }
    }
}

/// Wraps an accepted response with H1 return and dispatch ownership.
///
/// Successful CONNECT and `101` responses retire the H1 record before the
/// response exposes Hyper's upgraded root I/O.
fn guard_response(
    response: Response<hyper::body::Incoming>,
    method: Method,
    return_task: H1ReturnTask,
    dispatch: DispatchGuard,
    spawner: StdArc<dyn DriverSpawner>,
) -> Response<SdkBody> {
    let upgrade = response.status() == http_1x::StatusCode::SWITCHING_PROTOCOLS
        || (method == Method::CONNECT && response.status().is_success());
    let (parts, body) = response.into_parts();
    if upgrade {
        return_task.retire(CloseReason::Upgraded);
        dispatch.complete();
        return Response::from_parts(parts, SdkBody::from_body_1_x(body));
    }
    let body = H1ResponseBody::new(body, return_task, dispatch, spawner);
    Response::from_parts(parts, SdkBody::from_body_1_x(body))
}

/// Response body that owns accepted H1 cleanup through a message boundary.
struct H1ResponseBody {
    inner: hyper::body::Incoming,
    lifecycle: Option<H1ResponseLifecycle>,
}

impl H1ResponseBody {
    fn new(
        inner: hyper::body::Incoming,
        return_task: H1ReturnTask,
        dispatch: DispatchGuard,
        spawner: StdArc<dyn DriverSpawner>,
    ) -> Self {
        let mut body = Self {
            inner,
            lifecycle: Some(H1ResponseLifecycle {
                return_task: Some(return_task),
                dispatch: Some(dispatch),
                spawner,
            }),
        };
        if body.inner.is_end_stream() {
            body.finish_without_context();
        }
        body
    }

    fn finish_with_context(&mut self, cx: &mut Context<'_>) {
        if let Some(lifecycle) = self.lifecycle.take() {
            lifecycle.finish(Some(cx));
        }
    }

    fn finish_without_context(&mut self) {
        if let Some(lifecycle) = self.lifecycle.take() {
            lifecycle.finish(None);
        }
    }

    fn fail(&mut self) {
        if let Some(mut lifecycle) = self.lifecycle.take() {
            if let Some(return_task) = lifecycle.return_task.take() {
                return_task.retire(CloseReason::IncompleteH1Exchange);
            }
            drop(lifecycle.dispatch.take());
        }
    }
}

impl Body for H1ResponseBody {
    type Data = <hyper::body::Incoming as Body>::Data;
    type Error = <hyper::body::Incoming as Body>::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
        match Pin::new(&mut self.inner).poll_frame(cx) {
            Poll::Ready(None) => {
                self.finish_with_context(cx);
                Poll::Ready(None)
            }
            Poll::Ready(Some(Err(error))) => {
                self.fail();
                Poll::Ready(Some(Err(error)))
            }
            other => other,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        self.inner.size_hint()
    }
}

impl Drop for H1ResponseBody {
    fn drop(&mut self) {
        if self.lifecycle.is_some() && self.inner.is_end_stream() {
            self.finish_without_context();
        }
    }
}

/// Sender return work detached from a response body.
struct H1ResponseLifecycle {
    /// Sender retained until Hyper proves the exchange reusable.
    return_task: Option<H1ReturnTask>,
    /// Accepted-dispatch accounting completed with response cleanup.
    dispatch: Option<DispatchGuard>,
    /// Owner-runtime placement for readiness work that outlives the body.
    spawner: StdArc<dyn DriverSpawner>,
}

/// Owns sender cleanup while a detached readiness future is submitted.
struct H1ReadinessTask {
    /// Return task retired as owner-runtime shutdown if the future is dropped.
    return_task: Option<H1ReturnTask>,
}

impl H1ResponseLifecycle {
    fn finish(mut self, cx: Option<&mut Context<'_>>) {
        let mut return_task = self
            .return_task
            .take()
            .expect("HTTP/1 response lifecycle lost its return task");
        let ready = match cx {
            Some(cx) => return_task.poll_ready(cx),
            None if return_task.is_ready() => Poll::Ready(Ok(())),
            None => Poll::Pending,
        };
        match ready {
            Poll::Ready(Ok(())) => {
                if let Some(offer) = return_task.into_offer() {
                    offer.resolve();
                }
            }
            Poll::Ready(Err(_)) => {
                return_task.retire(CloseReason::ProtocolClosed);
            }
            Poll::Pending => {
                let mut readiness = H1ReadinessTask::new(return_task);
                self.spawner.spawn(Box::pin(async move {
                    match poll_fn(|cx| readiness.poll_ready(cx)).await {
                        Ok(()) => readiness.reuse(),
                        Err(_) => readiness.retire(CloseReason::ProtocolClosed),
                    }
                }));
            }
        }
        if let Some(dispatch) = self.dispatch.take() {
            dispatch.complete();
        }
    }
}

impl H1ReadinessTask {
    /// Arms owner-runtime fallback around one pending sender.
    fn new(return_task: H1ReturnTask) -> Self {
        Self {
            return_task: Some(return_task),
        }
    }

    /// Polls the retained sender for Hyper's reusable-boundary proof.
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), hyper::Error>> {
        self.return_task
            .as_mut()
            .expect("HTTP/1 readiness task consumed more than once")
            .poll_ready(cx)
    }

    /// Returns the ready sender through ordinary source arbitration.
    fn reuse(mut self) {
        let return_task = self
            .return_task
            .take()
            .expect("HTTP/1 readiness task consumed more than once");
        if let Some(offer) = return_task.into_offer() {
            offer.resolve();
        }
    }

    /// Retires the sender with a terminal readiness result.
    fn retire(mut self, reason: CloseReason) {
        self.return_task
            .take()
            .expect("HTTP/1 readiness task consumed more than once")
            .retire(reason);
    }
}

impl Drop for H1ReadinessTask {
    fn drop(&mut self) {
        if let Some(return_task) = self.return_task.take() {
            return_task.retire(CloseReason::OwnerRuntimeShutdown);
        }
    }
}

/// Adds the mandatory HTTP/1 Host header from the absolute request URI.
fn add_host_header(
    request: &mut Request<SdkBody>,
    absolute_uri: &Uri,
) -> Result<(), RequestPreparationError> {
    if request.headers().contains_key(http_1x::header::HOST) {
        return Ok(());
    }
    let authority = absolute_uri
        .authority()
        .ok_or(RequestPreparationError::MissingAuthority)?;
    let default_port = match absolute_uri.scheme_str() {
        Some("http") => Some(80),
        Some("https") => Some(443),
        _ => None,
    };
    let host = match absolute_uri.port_u16() {
        Some(port) if Some(port) != default_port => {
            format!("{}:{port}", authority.host())
        }
        _ => authority.host().to_string(),
    };
    let value = http_1x::HeaderValue::from_str(&host)
        .map_err(RequestPreparationError::InvalidHostHeader)?;
    request.headers_mut().insert(http_1x::header::HOST, value);
    Ok(())
}

/// Rejects request forms Hyper cannot route before selecting a protocol.
fn validate_request_before_acquisition(
    request: &Request<SdkBody>,
) -> Result<(), RequestPreparationError> {
    match request.version() {
        Version::HTTP_11 | Version::HTTP_2 => Ok(()),
        Version::HTTP_10 if request.method() == Method::CONNECT => {
            Err(RequestPreparationError::ConnectOverHttp10)
        }
        Version::HTTP_10 => Ok(()),
        version => Err(RequestPreparationError::UnsupportedVersion(version)),
    }
}

/// A request property that must be rejected before protocol acquisition.
#[derive(Debug)]
enum RequestPreparationError {
    /// The absolute URI did not contain the authority required for `Host`.
    MissingAuthority,
    /// The derived `Host` value was not a valid HTTP header value.
    InvalidHostHeader(http_1x::header::InvalidHeaderValue),
    /// HTTP/1.0 cannot represent a CONNECT request accepted by this client.
    ConnectOverHttp10,
    /// The request selected an HTTP version this client cannot dispatch.
    UnsupportedVersion(Version),
}

impl std::fmt::Display for RequestPreparationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingAuthority => f.write_str("request URI must contain an authority"),
            Self::InvalidHostHeader(_) => {
                f.write_str("request URI authority is not a valid Host header")
            }
            Self::ConnectOverHttp10 => f.write_str("CONNECT is not supported over HTTP/1.0"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported HTTP version: {version:?}")
            }
        }
    }
}

impl std::error::Error for RequestPreparationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidHostHeader(error) => Some(error),
            _ => None,
        }
    }
}

/// Rewrites an absolute URI into the request target required by HTTP/1.
fn rewrite_h1_request_target(request: &mut Request<SdkBody>, is_proxied: bool) {
    use http_1x::uri::Parts;

    if request.method() == Method::CONNECT {
        if let Some(authority) = request.uri().authority().cloned() {
            let mut parts = Parts::default();
            parts.authority = Some(authority);
            *request.uri_mut() = Uri::from_parts(parts).expect("request authority is a valid URI");
        }
        return;
    }
    if is_proxied {
        return;
    }
    let mut parts = Parts::default();
    parts.path_and_query = request.uri().path_and_query().cloned();
    *request.uri_mut() = Uri::from_parts(parts).expect("request path and query are a valid URI");
}

#[cfg(all(test, not(smithy_http_client_loom), feature = "rt-tokio"))]
mod tests {
    use super::*;
    use crate::client::pool::cell::h1::H1Sender;
    use crate::client::pool::connection::ConnectionInfo;
    use crate::client::pool::{
        Client, ConnectionId, ConnectionPool, Partition, PartitionId, TokioDriverSpawner,
    };
    use crate::client::timeout::test::{NeverConnects, NeverReplies};
    use aws_smithy_async::rt::sleep::TokioSleep;
    use aws_smithy_runtime_api::client::connection::CaptureSmithyConnection;
    use aws_smithy_runtime_api::client::http::{HttpClient, HttpConnector, HttpConnectorSettings};
    use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
    use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
    use http_body_util::BodyExt;
    use hyper_util::client::legacy::connect::{Connected, Connection};
    use hyper_util::rt::TokioIo;
    use std::future::Future;
    use std::io::{self, IoSlice};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::Notify;
    use tower::Service;

    #[derive(Debug)]
    struct DroppingSpawner;

    impl DriverSpawner for DroppingSpawner {
        fn spawn(&self, driver: Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>) {
            drop(driver);
        }
    }

    #[derive(Clone, Debug)]
    struct TrackingSpawner {
        active: StdArc<AtomicUsize>,
    }

    impl DriverSpawner for TrackingSpawner {
        fn spawn(&self, driver: Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>) {
            self.active.fetch_add(1, Ordering::SeqCst);
            let active = self.active.clone();
            tokio::spawn(async move {
                struct ActiveGuard(StdArc<AtomicUsize>);

                impl Drop for ActiveGuard {
                    fn drop(&mut self) {
                        self.0.fetch_sub(1, Ordering::SeqCst);
                    }
                }

                let _guard = ActiveGuard(active);
                driver.await;
            });
        }
    }

    #[derive(Clone, Debug)]
    struct CountingSpawner {
        submitted: StdArc<AtomicUsize>,
    }

    impl DriverSpawner for CountingSpawner {
        fn spawn(&self, driver: Pin<Box<dyn Future<Output = ()> + Send + 'static>>) {
            self.submitted.fetch_add(1, Ordering::SeqCst);
            drop(tokio::spawn(driver));
        }
    }

    #[derive(Clone, Debug)]
    struct PausedConnector {
        started: StdArc<Notify>,
        release: StdArc<Notify>,
        calls: StdArc<AtomicUsize>,
    }

    impl Service<Uri> for PausedConnector {
        type Response = TokioIo<TcpStream>;
        type Error = io::Error;
        type Future =
            Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, uri: Uri) -> Self::Future {
            let address = uri
                .authority()
                .expect("test URI has an authority")
                .as_str()
                .to_owned();
            let started = self.started.clone();
            let release = self.release.clone();
            let calls = self.calls.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                started.notify_one();
                release.notified().await;
                TcpStream::connect(address).await.map(TokioIo::new)
            })
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct ConnectorMarker(&'static str);

    struct ExtraIo(TokioIo<TcpStream>);

    impl hyper::rt::Read for ExtraIo {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: hyper::rt::ReadBufCursor<'_>,
        ) -> Poll<io::Result<()>> {
            hyper::rt::Read::poll_read(Pin::new(&mut self.0), cx, buf)
        }
    }

    impl hyper::rt::Write for ExtraIo {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            hyper::rt::Write::poll_write(Pin::new(&mut self.0), cx, buf)
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            hyper::rt::Write::poll_flush(Pin::new(&mut self.0), cx)
        }

        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            hyper::rt::Write::poll_shutdown(Pin::new(&mut self.0), cx)
        }

        fn is_write_vectored(&self) -> bool {
            hyper::rt::Write::is_write_vectored(&self.0)
        }

        fn poll_write_vectored(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bufs: &[IoSlice<'_>],
        ) -> Poll<io::Result<usize>> {
            hyper::rt::Write::poll_write_vectored(Pin::new(&mut self.0), cx, bufs)
        }
    }

    impl Connection for ExtraIo {
        fn connected(&self) -> Connected {
            Connected::new().extra(ConnectorMarker("connector-extra"))
        }
    }

    #[derive(Clone, Debug)]
    struct ExtraConnector;

    impl Service<Uri> for ExtraConnector {
        type Response = ExtraIo;
        type Error = io::Error;
        type Future =
            Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, uri: Uri) -> Self::Future {
            let address = uri
                .authority()
                .expect("test URI has an authority")
                .as_str()
                .to_owned();
            Box::pin(async move {
                TcpStream::connect(address)
                    .await
                    .map(TokioIo::new)
                    .map(ExtraIo)
            })
        }
    }

    #[test]
    fn dropped_readiness_task_records_owner_runtime_shutdown() {
        let pool = ConnectionPool::builder()
            .idle_timeout(None)
            .build_http()
            .unwrap();
        let partition = anonymous_partition(&pool);
        let uri = "http://example.com/".parse().unwrap();
        let cell = pool.inner.registry.resolve_cell(&partition, &uri).unwrap();
        let (connection, _physical) = ConnectionState::unbounded(ConnectionInfo::for_test(
            ConnectionId::new(1),
            PartitionId::ANONYMOUS,
        ));
        let selection =
            OriginCell::install_selected_h1(&cell, connection.clone(), H1Sender::test(11));

        drop(H1ReadinessTask::new(selection.into_return_task()));

        assert_eq!(
            Some(CloseReason::OwnerRuntimeShutdown),
            connection.snapshot().close_reason
        );
    }

    #[tokio::test]
    async fn dropped_owner_task_completes_the_establishment_waiter() {
        let pool = ConnectionPool::builder()
            .idle_timeout(None)
            .partitions([Partition::new(PartitionId::from_index(7), DroppingSpawner)])
            .build_http()
            .unwrap();
        let partition = pool
            .inner
            .registry
            .partition(PartitionId::from_index(7))
            .unwrap();
        let request = Request::get("http://127.0.0.1:9/")
            .body(SdkBody::empty())
            .unwrap();

        let error = pool
            .send_request(partition, request, None)
            .await
            .expect_err("dropped establishment task did not fail the request");
        assert!(error.is_other());
        assert_eq!(
            "the owner-runtime connection establishment task was dropped",
            std::error::Error::source(&error).unwrap().to_string()
        );
    }

    struct TestServer {
        endpoint: String,
        accepted: StdArc<AtomicUsize>,
        task: tokio::task::JoinHandle<()>,
    }

    impl TestServer {
        async fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let accepted = StdArc::new(AtomicUsize::new(0));
            let accepted_for_task = accepted.clone();
            let task = tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    accepted_for_task.fetch_add(1, Ordering::SeqCst);
                    tokio::spawn(serve_connection(stream));
                }
            });
            Self {
                endpoint,
                accepted,
                task,
            }
        }

        fn uri(&self, path: &str) -> Uri {
            format!("{}{}", self.endpoint, path).parse().unwrap()
        }

        fn accepted(&self) -> usize {
            self.accepted.load(Ordering::SeqCst)
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn serve_connection(mut stream: TcpStream) {
        let mut buffered = Vec::new();
        let mut read = [0_u8; 4096];
        loop {
            let count = match stream.read(&mut read).await {
                Ok(0) | Err(_) => return,
                Ok(count) => count,
            };
            buffered.extend_from_slice(&read[..count]);
            while let Some(end) = buffered.windows(4).position(|window| window == b"\r\n\r\n") {
                buffered.drain(..end + 4);
                if stream
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok")
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    }

    async fn read_request_head(stream: &mut TcpStream) -> bool {
        let mut buffered = Vec::new();
        let mut read = [0_u8; 1024];
        loop {
            let count = match stream.read(&mut read).await {
                Ok(0) | Err(_) => return false,
                Ok(count) => count,
            };
            buffered.extend_from_slice(&read[..count]);
            if buffered.windows(4).any(|window| window == b"\r\n\r\n") {
                return true;
            }
        }
    }

    fn anonymous_partition(pool: &ConnectionPool) -> Arc<PartitionState> {
        pool.inner
            .registry
            .partition(PartitionId::ANONYMOUS)
            .unwrap()
    }

    async fn request(
        pool: &ConnectionPool,
        partition: Arc<PartitionState>,
        uri: Uri,
    ) -> Response<SdkBody> {
        pool.send_request(
            partition,
            Request::get(uri).body(SdkBody::empty()).unwrap(),
            None,
        )
        .await
        .unwrap()
    }

    async fn consume(response: Response<SdkBody>) -> hyper::body::Bytes {
        response.into_body().collect().await.unwrap().to_bytes()
    }

    #[tokio::test]
    async fn anonymous_pool_sends_and_reuses_h1() {
        let server = TestServer::start().await;
        let pool = ConnectionPool::builder().build_http().unwrap();
        let partition = anonymous_partition(&pool);

        assert_eq!(
            hyper::body::Bytes::from_static(b"ok"),
            consume(request(&pool, partition.clone(), server.uri("/one")).await).await
        );
        assert_eq!(
            hyper::body::Bytes::from_static(b"ok"),
            consume(request(&pool, partition, server.uri("/two")).await).await
        );
        assert_eq!(1, server.accepted());
    }

    #[tokio::test]
    async fn connector_extras_reach_the_returned_response() {
        let server = TestServer::start().await;
        let pool = super::super::builder::Builder::default()
            .idle_timeout(None)
            .build_with_connector(ExtraConnector)
            .unwrap();
        let partition = anonymous_partition(&pool);

        let response = request(&pool, partition, server.uri("/extras")).await;

        assert_eq!(
            Some(&ConnectorMarker("connector-extra")),
            response.extensions().get::<ConnectorMarker>()
        );
        consume(response).await;
    }

    #[tokio::test]
    async fn ready_sender_finishes_without_a_readiness_task() {
        let server = TestServer::start().await;
        let submitted = StdArc::new(AtomicUsize::new(0));
        let pool = ConnectionPool::builder()
            .idle_timeout(None)
            .partitions([Partition::new(
                PartitionId::from_index(7),
                CountingSpawner {
                    submitted: submitted.clone(),
                },
            )])
            .build_http()
            .unwrap();
        let partition = pool
            .inner
            .registry
            .partition(PartitionId::from_index(7))
            .unwrap();
        let uri = server.uri("/ready");
        consume(request(&pool, partition.clone(), uri.clone()).await).await;
        let baseline = submitted.load(Ordering::SeqCst);
        let cell = pool.inner.registry.resolve_cell(&partition, &uri).unwrap();
        let selection = OriginCell::select_h1(&cell).expect("ready sender was not reusable");
        let connection = selection.connection().clone();
        let dispatch =
            ConnectionState::try_commit_dispatch(&connection).expect("connection was not open");
        let return_task = selection.into_return_task();
        assert!(return_task.is_ready());

        H1ResponseLifecycle {
            return_task: Some(return_task),
            dispatch: Some(dispatch),
            spawner: StdArc::new(CountingSpawner {
                submitted: submitted.clone(),
            }),
        }
        .finish(None);

        assert_eq!(baseline, submitted.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn cancelling_launched_establishment_returns_its_connection_to_a_later_waiter() {
        let server = TestServer::start().await;
        let started = StdArc::new(Notify::new());
        let release = StdArc::new(Notify::new());
        let calls = StdArc::new(AtomicUsize::new(0));
        let pool = super::super::builder::Builder::default()
            .idle_timeout(None)
            .max_connections_per_host(1)
            .build_with_connector(PausedConnector {
                started: started.clone(),
                release: release.clone(),
                calls: calls.clone(),
            })
            .unwrap();
        let partition = anonymous_partition(&pool);
        let first_pool = pool.clone();
        let first_partition = partition.clone();
        let first_uri = server.uri("/cancelled-establishment");
        let first = tokio::spawn(async move {
            first_pool
                .send_request(
                    first_partition,
                    Request::get(first_uri).body(SdkBody::empty()).unwrap(),
                    None,
                )
                .await
        });
        started.notified().await;

        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        release.notify_one();

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            request(
                &pool,
                partition,
                server.uri("/after-cancelled-establishment"),
            ),
        )
        .await
        .expect("completed establishment did not satisfy the later waiter");
        consume(response).await;
        assert_eq!(1, calls.load(Ordering::SeqCst));
        assert_eq!(1, server.accepted());
    }

    #[tokio::test]
    async fn cancelling_an_accepted_exchange_retires_its_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let accepted = StdArc::new(AtomicUsize::new(0));
        let accepted_for_task = accepted.clone();
        let first_request = StdArc::new(Notify::new());
        let first_request_for_task = first_request.clone();
        let server = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let index = accepted_for_task.fetch_add(1, Ordering::SeqCst);
                if index == 0 {
                    let first_request = first_request_for_task.clone();
                    tokio::spawn(async move {
                        assert!(read_request_head(&mut stream).await);
                        first_request.notify_one();
                        let mut sink = [0_u8; 32];
                        while stream.read(&mut sink).await.unwrap_or(0) != 0 {}
                    });
                } else {
                    tokio::spawn(serve_connection(stream));
                }
            }
        });
        let pool = ConnectionPool::builder()
            .idle_timeout(None)
            .max_connections_per_host(1)
            .build_http()
            .unwrap();
        let partition = anonymous_partition(&pool);
        let first_pool = pool.clone();
        let first_partition = partition.clone();
        let first_uri: Uri = format!("{endpoint}/cancelled-exchange").parse().unwrap();
        let first = tokio::spawn(async move {
            first_pool
                .send_request(
                    first_partition,
                    Request::get(first_uri).body(SdkBody::empty()).unwrap(),
                    None,
                )
                .await
        });
        first_request.notified().await;
        let cell = pool
            .inner
            .registry
            .resolve_cell(&partition, &format!("{endpoint}/").parse().unwrap())
            .unwrap();
        let connection = cell.only_h1_connection_for_test();

        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        assert_eq!(
            Some(CloseReason::IncompleteH1Exchange),
            connection.snapshot().close_reason
        );

        let second_uri: Uri = format!("{endpoint}/after-cancelled-exchange")
            .parse()
            .unwrap();
        let second = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            request(&pool, partition, second_uri),
        )
        .await
        .expect("accepted-exchange cancellation did not release bounded capacity");
        consume(second).await;
        assert_eq!(2, accepted.load(Ordering::SeqCst));
        server.abort();
    }

    #[tokio::test]
    async fn bounded_second_request_waits_for_h1_return() {
        let server = TestServer::start().await;
        let pool = ConnectionPool::builder()
            .max_connections_per_host(1)
            .build_http()
            .unwrap();
        let partition = anonymous_partition(&pool);
        let first = request(&pool, partition.clone(), server.uri("/first")).await;

        let second_pool = pool.clone();
        let second_partition = partition.clone();
        let second_uri = server.uri("/second");
        let second =
            tokio::spawn(async move { request(&second_pool, second_partition, second_uri).await });
        tokio::task::yield_now().await;
        assert_eq!(1, server.accepted());
        assert!(!second.is_finished());

        assert_eq!(hyper::body::Bytes::from_static(b"ok"), consume(first).await);
        assert_eq!(
            hyper::body::Bytes::from_static(b"ok"),
            consume(second.await.unwrap()).await
        );
        assert_eq!(1, server.accepted());
    }

    #[tokio::test]
    async fn dropping_a_queued_request_cancels_its_waiter() {
        let server = TestServer::start().await;
        let pool = ConnectionPool::builder()
            .max_connections_per_host(1)
            .build_http()
            .unwrap();
        let partition = anonymous_partition(&pool);
        let first = request(&pool, partition.clone(), server.uri("/first")).await;
        let second_uri = server.uri("/second");
        let cell = pool
            .inner
            .registry
            .resolve_cell(&partition, &second_uri)
            .unwrap();
        let second_pool = pool.clone();
        let second_partition = partition.clone();
        let second =
            tokio::spawn(async move { request(&second_pool, second_partition, second_uri).await });
        for _ in 0..10 {
            if cell.retained_waiters_for_test() == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(1, cell.retained_waiters_for_test());

        second.abort();
        assert!(second.await.unwrap_err().is_cancelled());
        for _ in 0..10 {
            if cell.retained_waiters_for_test() == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(0, cell.retained_waiters_for_test());
        assert_eq!(0, cell.admission().unwrap().ordered_demand_count_for_test());

        consume(first).await;
        consume(request(&pool, partition, server.uri("/third")).await).await;
        assert_eq!(1, server.accepted());
    }

    #[tokio::test]
    async fn dropping_incomplete_body_retires_connection() {
        let server = TestServer::start().await;
        let pool = ConnectionPool::builder().build_http().unwrap();
        let partition = anonymous_partition(&pool);

        drop(request(&pool, partition.clone(), server.uri("/first")).await);
        assert_eq!(
            hyper::body::Bytes::from_static(b"ok"),
            consume(request(&pool, partition, server.uri("/second")).await).await
        );
        assert_eq!(2, server.accepted());
    }

    #[tokio::test]
    async fn incompatible_h1_selection_reports_connection_identity() {
        let server = TestServer::start().await;
        let pool = ConnectionPool::builder().build_http().unwrap();
        let partition = anonymous_partition(&pool);
        let capture = CaptureSmithyConnection::new();
        let mut request = Request::get(server.uri("/h2"))
            .version(Version::HTTP_2)
            .body(SdkBody::empty())
            .unwrap();
        request.extensions_mut().insert(capture.clone());

        let error = pool
            .send_request(partition, request, None)
            .await
            .expect_err("an HTTP/2 request unexpectedly used HTTP/1");
        assert!(error.is_user());
        assert!(error
            .connection_metadata()
            .and_then(|metadata| metadata.connection_id())
            .is_some());
        assert!(capture
            .get()
            .and_then(|metadata| metadata.connection_id())
            .is_some());
    }

    #[tokio::test]
    async fn captured_metadata_poisons_only_selected_generation() {
        let server = TestServer::start().await;
        let pool = ConnectionPool::builder().build_http().unwrap();
        let partition = anonymous_partition(&pool);
        let capture = CaptureSmithyConnection::new();
        let mut first = Request::get(server.uri("/first"))
            .body(SdkBody::empty())
            .unwrap();
        first.extensions_mut().insert(capture.clone());
        let response = pool
            .send_request(partition.clone(), first, None)
            .await
            .unwrap();
        consume(response).await;

        let metadata = capture.get().expect("connection metadata was captured");
        assert!(metadata.connection_id().is_some());
        metadata.poison();

        consume(request(&pool, partition.clone(), server.uri("/second")).await).await;
        assert_eq!(2, server.accepted());

        metadata.poison();
        consume(request(&pool, partition, server.uri("/third")).await).await;
        assert_eq!(
            2,
            server.accepted(),
            "stale metadata poisoned the newer connection generation"
        );
    }

    #[tokio::test]
    async fn connection_id_exhaustion_is_an_internal_error() {
        let server = TestServer::start().await;
        let pool = ConnectionPool::builder().build_http().unwrap();
        pool.inner
            .next_connection_id
            .store(u64::MAX, Ordering::Relaxed);
        let partition = anonymous_partition(&pool);
        let request = Request::get(server.uri("/id-exhausted"))
            .body(SdkBody::empty())
            .unwrap();

        let error = pool
            .send_request(partition, request, None)
            .await
            .expect_err("exhausted connection identifiers unexpectedly established a connection");
        assert!(error.is_other());
        assert_eq!(
            "connection identifier space exhausted",
            std::error::Error::source(&error).unwrap().to_string()
        );
    }

    #[tokio::test]
    async fn explicit_clients_resolve_declared_partitions() {
        let pool = ConnectionPool::builder()
            .partitions([Partition::new(
                PartitionId::from_index(7),
                TokioDriverSpawner::current(),
            )])
            .build_http()
            .unwrap();

        assert!(Client::new(&pool).is_err());
        assert!(Client::from_partition(&pool, PartitionId::from_index(7)).is_ok());
        let error = Client::from_partition(&pool, PartitionId::from_index(8)).unwrap_err();
        assert_eq!(PartitionId::from_index(8), error.partition());
    }

    #[test]
    fn anonymous_partition_outside_tokio_is_a_user_error() {
        let pool = ConnectionPool::builder()
            .idle_timeout(None)
            .build_http()
            .unwrap();
        let partition = anonymous_partition(&pool);
        let request = Request::get("http://example.com/")
            .body(SdkBody::empty())
            .unwrap();
        let mut send = Box::pin(pool.send_request(partition, request, None));
        let waker = std::task::Waker::noop();
        let mut context = Context::from_waker(waker);

        let Poll::Ready(Err(error)) = send.as_mut().poll(&mut context) else {
            panic!("missing anonymous runtime did not fail on first poll");
        };
        assert!(error.is_user());
        assert_eq!(
            "the anonymous connection-pool partition requires an active Tokio runtime on first use",
            std::error::Error::source(&error).unwrap().to_string()
        );
    }

    #[tokio::test]
    async fn registry_shutdown_stops_partition_maintenance() {
        let active = StdArc::new(AtomicUsize::new(0));
        let pool = ConnectionPool::builder()
            .partitions([Partition::new(
                PartitionId::from_index(7),
                TrackingSpawner {
                    active: active.clone(),
                },
            )])
            .build_http()
            .unwrap();
        let partition = pool
            .inner
            .registry
            .partition(PartitionId::from_index(7))
            .unwrap();
        let spawner = partition.driver_spawner().unwrap();
        partition.start_maintenance(spawner.as_ref());
        for _ in 0..10 {
            if active.load(Ordering::SeqCst) == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(1, active.load(Ordering::SeqCst));

        pool.inner.registry.close_all(CloseReason::PoolDropped);
        for _ in 0..10 {
            if active.load(Ordering::SeqCst) == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(0, active.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn explicit_partition_submits_work_to_its_owner_runtime() {
        #[derive(Clone, Debug)]
        struct RecordingSpawner {
            handle: tokio::runtime::Handle,
            expected: tokio::runtime::Id,
            observed: StdArc<AtomicUsize>,
        }

        impl DriverSpawner for RecordingSpawner {
            fn spawn(
                &self,
                driver: Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>,
            ) {
                let expected = self.expected;
                let observed = self.observed.clone();
                drop(self.handle.spawn(async move {
                    assert_eq!(expected, tokio::runtime::Handle::current().id());
                    observed.fetch_add(1, Ordering::SeqCst);
                    driver.await;
                }));
            }
        }

        let server = TestServer::start().await;
        let owner = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let observed = StdArc::new(AtomicUsize::new(0));
        let pool = ConnectionPool::builder()
            .partitions([Partition::new(
                PartitionId::from_index(7),
                RecordingSpawner {
                    handle: owner.handle().clone(),
                    expected: owner.handle().id(),
                    observed: observed.clone(),
                },
            )])
            .build_http()
            .unwrap();
        let partition = pool
            .inner
            .registry
            .partition(PartitionId::from_index(7))
            .unwrap();

        consume(request(&pool, partition, server.uri("/explicit")).await).await;
        assert!(
            observed.load(Ordering::SeqCst) >= 2,
            "establishment and protocol-driver work did not run on the owner runtime"
        );
        drop(pool);
        owner.shutdown_background();
    }

    #[tokio::test]
    async fn smithy_client_boundary_sends_through_the_shared_pool() {
        let server = TestServer::start().await;
        let pool = ConnectionPool::builder().build_http().unwrap();
        let client = Client::new(&pool).unwrap();
        let components = RuntimeComponentsBuilder::for_tests().build().unwrap();
        let connector =
            client.http_connector(&HttpConnectorSettings::builder().build(), &components);

        let mut response = connector
            .call(HttpRequest::get(server.uri("/smithy").to_string()).unwrap())
            .await
            .unwrap();
        let body = response.take_body().collect().await.unwrap().to_bytes();
        assert_eq!(hyper::body::Bytes::from_static(b"ok"), body);
    }

    #[tokio::test]
    async fn connect_timeout_covers_the_transport_operation() {
        let pool = super::super::builder::Builder::default()
            .build_with_connector(NeverConnects)
            .unwrap();
        let client = Client::new(&pool).unwrap();
        let components = RuntimeComponentsBuilder::for_tests()
            .with_sleep_impl(Some(TokioSleep::new()))
            .build()
            .unwrap();
        let settings = HttpConnectorSettings::builder()
            .connect_timeout(std::time::Duration::from_millis(10))
            .build();

        let error = client
            .http_connector(&settings, &components)
            .call(HttpRequest::get("http://example.com/").unwrap())
            .await
            .unwrap_err();
        assert!(error.is_timeout(), "unexpected connector error: {error:?}");
    }

    #[tokio::test]
    async fn read_timeout_covers_handshake_through_response_headers() {
        let pool = super::super::builder::Builder::default()
            .build_with_connector(NeverReplies)
            .unwrap();
        let client = Client::new(&pool).unwrap();
        let components = RuntimeComponentsBuilder::for_tests()
            .with_sleep_impl(Some(TokioSleep::new()))
            .build()
            .unwrap();
        let settings = HttpConnectorSettings::builder()
            .read_timeout(std::time::Duration::from_millis(10))
            .build();

        let error = client
            .http_connector(&settings, &components)
            .call(HttpRequest::get("http://example.com/").unwrap())
            .await
            .unwrap_err();
        assert!(error.is_timeout(), "unexpected connector error: {error:?}");
    }

    async fn assert_h1_upgrade_releases_capacity(
        method: Method,
        response_head: &'static [u8],
        expected_status: http_1x::StatusCode,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let accepted = StdArc::new(AtomicUsize::new(0));
        let accepted_for_task = accepted.clone();
        let request_received = StdArc::new(Notify::new());
        let request_received_for_task = request_received.clone();
        let release_response = StdArc::new(Notify::new());
        let release_response_for_task = release_response.clone();
        let server = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let index = accepted_for_task.fetch_add(1, Ordering::SeqCst);
                if index == 0 {
                    let request_received = request_received_for_task.clone();
                    let release_response = release_response_for_task.clone();
                    tokio::spawn(async move {
                        if !read_request_head(&mut stream).await {
                            return;
                        }
                        request_received.notify_one();
                        release_response.notified().await;
                        stream.write_all(response_head).await.unwrap();
                        let mut sink = [0_u8; 32];
                        while stream.read(&mut sink).await.unwrap_or(0) != 0 {}
                    });
                } else {
                    tokio::spawn(serve_connection(stream));
                }
            }
        });

        let pool = ConnectionPool::builder()
            .max_connections_per_host(1)
            .build_http()
            .unwrap();
        let partition = anonymous_partition(&pool);
        let upgrade_uri: Uri = format!("{endpoint}/upgrade").parse().unwrap();
        let mut upgrade_request = Request::builder()
            .method(method.clone())
            .uri(upgrade_uri.clone())
            .body(SdkBody::empty())
            .unwrap();
        if method != Method::CONNECT {
            upgrade_request
                .headers_mut()
                .insert(http_1x::header::CONNECTION, "upgrade".parse().unwrap());
            upgrade_request
                .headers_mut()
                .insert(http_1x::header::UPGRADE, "test".parse().unwrap());
        }
        let request_pool = pool.clone();
        let request_partition = partition.clone();
        let response = tokio::spawn(async move {
            request_pool
                .send_request(request_partition, upgrade_request, None)
                .await
                .unwrap()
        });
        request_received.notified().await;
        let cell = pool
            .inner
            .registry
            .resolve_cell(&partition, &upgrade_uri)
            .unwrap();
        let connection = cell.only_h1_connection_for_test();
        release_response.notify_one();
        let mut response = response.await.unwrap();
        assert_eq!(expected_status, response.status());

        let upgraded = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            hyper::upgrade::on(&mut response),
        )
        .await
        .expect("upgrade timed out")
        .expect("upgrade failed");
        let mut upgraded = hyper_util::rt::TokioIo::new(upgraded);
        let mut greeting = [0_u8; 5];
        upgraded.read_exact(&mut greeting).await.unwrap();
        assert_eq!(b"hello", &greeting);

        assert_eq!(
            Some(CloseReason::Upgraded),
            connection.snapshot().close_reason,
            "the upgraded HTTP/1 sender returned to pool policy"
        );
        assert_eq!(1, cell.admission().unwrap().available_capacity_for_test());

        let second_uri: Uri = format!("{endpoint}/after").parse().unwrap();
        let second = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            request(&pool, partition, second_uri),
        )
        .await
        .expect("bounded capacity was not released at upgrade logical close");
        consume(second).await;
        assert_eq!(2, accepted.load(Ordering::SeqCst));

        drop(upgraded);
        drop(response);
        server.abort();
    }

    #[tokio::test]
    async fn switching_protocols_upgrade_releases_capacity_and_transfers_io() {
        assert_h1_upgrade_releases_capacity(
            Method::GET,
            b"HTTP/1.1 101 Switching Protocols\r\n\
              connection: upgrade\r\n\
              upgrade: test\r\n\r\nhello",
            http_1x::StatusCode::SWITCHING_PROTOCOLS,
        )
        .await;
    }

    #[tokio::test]
    async fn successful_connect_releases_capacity_and_transfers_io() {
        assert_h1_upgrade_releases_capacity(
            Method::CONNECT,
            b"HTTP/1.1 200 Connection Established\r\n\r\nhello",
            http_1x::StatusCode::OK,
        )
        .await;
    }
}
