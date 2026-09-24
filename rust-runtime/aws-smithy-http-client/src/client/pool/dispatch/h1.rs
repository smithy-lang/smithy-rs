/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP/1 acquisition, exclusive dispatch, and response completion.
//!
//! The parent dispatcher has already validated the request, resolved its
//! origin cell, and selected the partition runtime. This module prepares the
//! HTTP/1 wire form, acquires Hyper's exclusive request sender, and owns that
//! sender until the response reaches a reusable message boundary. Every
//! cancellation path either returns the sender to its connection-owning cell
//! or retires the connection.

use super::super::admission::ProtocolRequirement;
use super::super::cell::h1::{H1Exchange, H1Selection};
use super::super::cell::{AcquisitionEvent, AcquisitionResult, OriginCell, WaiterId};
use super::super::connection::{CloseReason, ConnectionState, DispatchGuard};
use super::super::establish;
use super::super::partition::DriverSpawner;
use super::super::ConnectionPool;
use super::{AcquisitionContext, RequestDispatchError};
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
    /// Acquires an HTTP/1 connection and sends one request.
    ///
    /// The reacquisition loop runs only while the pool still owns the request. A
    /// selection that closes before dispatch can be replaced immediately.
    /// Hyper may also return an unconsumed request when a reused connection
    /// rejects it before writing. Once Hyper accepts a request on a fresh
    /// connection, or begins an exchange on any connection, this function does
    /// not reacquire.
    pub(super) async fn send_h1_request(
        &self,
        context: AcquisitionContext,
        mut request: Request<SdkBody>,
    ) -> Result<Response<SdkBody>, ConnectorError> {
        let request_method = request.method().clone();
        // Add the HTTP/1 header that depends on the absolute request URI.
        add_host_header(&mut request, &context.absolute_uri)
            .map_err(|error| ConnectorError::user(error.into()))?;

        // Redispatch is valid only when the original request is recovered intact.
        loop {
            let mut selection = Self::acquire_h1(&context).await?;
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

            // Commit against logical close before transferring the request to
            // Hyper. A close that wins this race leaves the request untouched.
            let Some(dispatch) = ConnectionState::try_commit_dispatch(&connection) else {
                tracing::trace!(
                    connection_id = %connection.id(),
                    request_partition = ?context.partition.id(),
                    connection_partition = ?connection.owner_partition(),
                    origin_scheme = %connection.info().origin().scheme(),
                    origin_host = connection.info().origin().host(),
                    origin_port = ?connection.info().origin().port(),
                    "HTTP/1 selection became stale before dispatch; reacquiring"
                );
                *request.uri_mut() = context.absolute_uri.clone();
                selection.retire_connection(CloseReason::ProtocolClosed);
                continue;
            };
            let send = selection.sender_mut().hyper_mut().try_send_request(request);

            let exchange = selection.into_exchange();
            match send.await {
                Ok(mut response) => {
                    // Response-body ownership keeps both the accepted dispatch
                    // and exclusive request handle out of the pool until Hyper
                    // proves a complete message boundary.
                    connection
                        .info()
                        .apply_connector_extras(response.extensions_mut());
                    return Ok(guard_response(
                        response,
                        request_method,
                        exchange,
                        dispatch,
                        context.owner_spawner.clone(),
                    ));
                }
                Err(mut error) => {
                    if let Some(mut returned) = error.take_message() {
                        exchange.retire_connection(CloseReason::ProtocolClosed);
                        drop(dispatch);
                        if reused {
                            tracing::trace!(
                                connection_id = %connection.id(),
                                request_partition = ?context.partition.id(),
                                connection_partition = ?connection.owner_partition(),
                                origin_scheme = %connection.info().origin().scheme(),
                                origin_host = connection.info().origin().host(),
                                origin_port = ?connection.info().origin().port(),
                                "reused HTTP/1 connection rejected request; reacquiring"
                            );
                            *returned.uri_mut() = context.absolute_uri.clone();
                            // Hyper returned the original request without
                            // consuming its body, so reacquisition is authoritative.
                            request = returned;
                            continue;
                        }
                        let metadata = captured_metadata
                            .unwrap_or_else(|| connection.info().metadata(close_handle));
                        return Err(super::super::super::downcast_error(Box::new(
                            error.into_error(),
                        ))
                        .with_connection(metadata));
                    }
                    exchange.retire_connection(CloseReason::IncompleteH1Exchange);
                    drop(dispatch);
                    let metadata = captured_metadata
                        .unwrap_or_else(|| connection.info().metadata(close_handle));
                    return Err(
                        super::super::super::downcast_error(Box::new(error.into_error()))
                            .with_connection(metadata),
                    );
                }
            }
        }
    }

    /// Returns one exclusive HTTP/1 request handle for this request.
    ///
    /// A local idle handle completes immediately. Otherwise one cell-local
    /// waiter remains registered while it receives either a reusable handle,
    /// an establishment failure, or capacity to start establishment. Starting
    /// establishment does not complete the waiter: a returning connection may
    /// still satisfy it first.
    async fn acquire_h1(context: &AcquisitionContext) -> Result<H1Selection, ConnectorError> {
        if let Some(selection) = OriginCell::select_h1(&context.cell) {
            tracing::trace!(
                connection_id = %selection.connection_id(),
                request_partition = ?context.partition.id(),
                connection_partition = ?selection.connection().owner_partition(),
                origin_scheme = %selection.connection().info().origin().scheme(),
                origin_host = selection.connection().info().origin().host(),
                origin_port = ?selection.connection().info().origin().port(),
                "HTTP/1 pool hit; reusing idle connection"
            );
            return Ok(selection);
        }

        let waiter = context
            .cell
            .register_waiter(ProtocolRequirement::H1Compatible);
        tracing::trace!(
            request_partition = ?context.partition.id(),
            origin_scheme = %context.cell.id().origin().scheme(),
            origin_host = context.cell.id().origin().host(),
            origin_port = ?context.cell.id().origin().port(),
            "HTTP/1 pool miss; acquisition queued"
        );
        let mut waiter_guard = WaiterGuard::new(context.cell.clone(), waiter);
        loop {
            match poll_fn(|cx| context.cell.poll_waiter(waiter, cx)).await {
                AcquisitionEvent::Complete(AcquisitionResult::H1(selection)) => {
                    tracing::trace!(
                        connection_id = %selection.connection_id(),
                        request_partition = ?context.partition.id(),
                        connection_partition = ?selection.connection().owner_partition(),
                        origin_scheme = %selection.connection().info().origin().scheme(),
                        origin_host = selection.connection().info().origin().host(),
                        origin_port = ?selection.connection().info().origin().port(),
                        "HTTP/1 waiter acquired a reusable connection"
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
                        request_partition = ?context.partition.id(),
                        origin_scheme = %context.cell.id().origin().scheme(),
                        origin_host = context.cell.id().origin().host(),
                        origin_port = ?context.cell.id().origin().port(),
                        "HTTP/1 waiter starting connection establishment"
                    );
                    let attempt = establish::establish_h1(context.clone(), permit);
                    let completion = EstablishmentCompletion::new(context.cell.clone(), waiter);
                    context.owner_spawner.spawn(Box::pin(async move {
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

/// Guarantees a terminal result for one submitted establishment attempt.
///
/// This guard moves into the owner-runtime future before it is submitted. If
/// the runtime discards that future, [`Drop`] reports a failure to the waiter.
/// If another connection satisfies the waiter before the submitted future
/// starts, [`Self::start`] rejects the attempt and the guard is disarmed.
struct EstablishmentCompletion {
    /// Cell retaining the waiter and establishment state.
    cell: Arc<OriginCell>,
    /// Waiter that receives the attempt's terminal result.
    waiter: WaiterId,
    /// Whether task drop still owes a failure result.
    active: bool,
}

impl EstablishmentCompletion {
    /// Arms task-drop completion before the future is submitted.
    fn new(cell: Arc<OriginCell>, waiter: WaiterId) -> Self {
        Self {
            cell,
            waiter,
            active: true,
        }
    }

    /// Marks the waiter started immediately before the connector is first polled.
    fn start(&self) -> bool {
        self.cell.start_establishment(self.waiter)
    }

    /// Delivers the attempt result and disarms task-drop completion.
    fn complete(mut self, result: AcquisitionResult) {
        self.active = false;
        self.cell.complete_establishment(self.waiter, result);
    }

    /// Disarms after another connection has already completed the waiter.
    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for EstablishmentCompletion {
    fn drop(&mut self) {
        if self.active {
            let error = ConnectorError::other(EstablishmentTaskDropped.into(), None);
            self.cell
                .complete_establishment(self.waiter, AcquisitionResult::Failed(error));
            tracing::debug!(
                request_partition = ?self.cell.id().partition(),
                connection_partition = ?self.cell.id().partition(),
                origin_scheme = %self.cell.id().origin().scheme(),
                origin_host = self.cell.id().origin().host(),
                origin_port = ?self.cell.id().origin().port(),
                "HTTP/1 connection establishment task dropped"
            );
        }
    }
}

/// The owner runtime discarded an establishment future before completion.
#[derive(Debug)]
struct EstablishmentTaskDropped;

impl std::fmt::Display for EstablishmentTaskDropped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the owner-runtime connection establishment task was dropped")
    }
}

impl std::error::Error for EstablishmentTaskDropped {}

/// Cancels a request's retained waiter if acquisition does not complete.
///
/// The request future owns this guard from waiter registration until it
/// consumes a terminal result. Dropping the future cancels the waiter; cell
/// cancellation returns any crossing capacity or HTTP/1 handle after releasing
/// the cell lock.
struct WaiterGuard {
    /// Cell retaining the waiter.
    cell: Arc<OriginCell>,
    /// Waiter cancelled if the request future does not consume a result.
    waiter: WaiterId,
    /// Whether request cancellation is still armed.
    active: bool,
}

impl WaiterGuard {
    /// Arms cancellation for a newly registered waiter.
    fn new(cell: Arc<OriginCell>, waiter: WaiterId) -> Self {
        Self {
            cell,
            waiter,
            active: true,
        }
    }

    /// Disarms after the request consumes its terminal result.
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

/// Attaches connection return and dispatch completion to an accepted response.
///
/// An ordinary response retains both guards until Hyper proves a complete
/// message boundary. Successful CONNECT and `101` responses retire the pool
/// record before the response exposes Hyper's upgraded root I/O.
fn guard_response(
    response: Response<hyper::body::Incoming>,
    method: Method,
    exchange: H1Exchange,
    dispatch: DispatchGuard,
    spawner: StdArc<dyn DriverSpawner>,
) -> Response<SdkBody> {
    let upgrade = response.status() == http_1x::StatusCode::SWITCHING_PROTOCOLS
        || (method == Method::CONNECT && response.status().is_success());
    let (parts, body) = response.into_parts();
    if upgrade {
        exchange.retire_connection(CloseReason::Upgraded);
        dispatch.complete();
        return Response::from_parts(parts, SdkBody::from_body_1_x(body));
    }
    let body = H1ResponseBody::new(body, exchange, dispatch, spawner);
    Response::from_parts(parts, SdkBody::from_body_1_x(body))
}

/// Response body that retains the HTTP/1 exchange until its message boundary.
///
/// End-of-stream returns a ready request handle to pool policy. A body error,
/// or dropping the body before Hyper reports end-of-stream, retires the
/// connection instead.
struct H1ResponseBody {
    /// Hyper response body whose terminal state determines message completion.
    inner: hyper::body::Incoming,
    /// Sender and dispatch ownership until one terminal body observation.
    lifecycle: Option<H1ResponseLifecycle>,
}

impl H1ResponseBody {
    /// Wraps a response and immediately completes a body already at end stream.
    fn new(
        inner: hyper::body::Incoming,
        exchange: H1Exchange,
        dispatch: DispatchGuard,
        spawner: StdArc<dyn DriverSpawner>,
    ) -> Self {
        let mut body = Self {
            inner,
            lifecycle: Some(H1ResponseLifecycle {
                exchange: Some(exchange),
                dispatch: Some(dispatch),
                spawner,
            }),
        };
        if body.inner.is_end_stream() {
            body.finish_without_context();
        }
        body
    }

    /// Completes response ownership using the current body-task waker.
    fn finish_with_context(&mut self, cx: &mut Context<'_>) {
        if let Some(lifecycle) = self.lifecycle.take() {
            lifecycle.finish(Some(cx));
        }
    }

    /// Completes response ownership without a task context when already ready.
    fn finish_without_context(&mut self) {
        if let Some(lifecycle) = self.lifecycle.take() {
            lifecycle.finish(None);
        }
    }

    /// Retires an exchange whose response body ended with an error.
    fn fail(&mut self) {
        if let Some(mut lifecycle) = self.lifecycle.take() {
            if let Some(exchange) = lifecycle.exchange.take() {
                exchange.retire_connection(CloseReason::IncompleteH1Exchange);
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

/// Ownership retained from response headers through the HTTP/1 message boundary.
///
/// The response body owns this value together with the accepted exchange and
/// dispatch accounting. End-of-stream polls Hyper readiness: success begins
/// ordinary sender return, failure retires the connection, and pending
/// readiness transfers the exchange to [`H1ReadinessTask`] on the
/// connection-owning runtime. Dropping it before completion drops
/// [`H1Exchange`] and [`DispatchGuard`], retiring the incomplete exchange and
/// completing dispatch accounting through their fallbacks.
struct H1ResponseLifecycle {
    /// Exclusive request handle retained until Hyper proves it reusable.
    exchange: Option<H1Exchange>,
    /// Accepted-dispatch accounting completed with response cleanup.
    dispatch: Option<DispatchGuard>,
    /// Owner-runtime placement for readiness work that outlives the body.
    spawner: StdArc<dyn DriverSpawner>,
}

/// Owner-runtime task waiting for Hyper to prove the sender reusable.
///
/// [`H1ResponseLifecycle`] creates this only after the response reaches
/// end-of-stream while sender readiness is still pending. Readiness success
/// returns the sender through [`H1Exchange::offer_for_reuse`]; readiness failure
/// retires the connection. Dropping the submitted task before either result retires the
/// exchange as [`CloseReason::OwnerRuntimeShutdown`], so an unproven sender can
/// never re-enter the pool.
struct H1ReadinessTask {
    /// Exchange retired as owner-runtime shutdown if the task is dropped.
    exchange: Option<H1Exchange>,
}

impl H1ResponseLifecycle {
    /// Returns or retires the request handle and completes dispatch accounting.
    ///
    /// Readiness is first polled with the response body's task context when one
    /// is available. Pending readiness moves to an owner-runtime task so the
    /// completed body does not retain connection ownership.
    fn finish(mut self, cx: Option<&mut Context<'_>>) {
        let mut exchange = self
            .exchange
            .take()
            .expect("HTTP/1 response lifecycle lost its exchange");
        let ready = match cx {
            Some(cx) => exchange.poll_ready(cx),
            None if exchange.is_ready() => Poll::Ready(Ok(())),
            None => Poll::Pending,
        };
        match ready {
            Poll::Ready(Ok(())) => {
                exchange.offer_for_reuse();
            }
            Poll::Ready(Err(_)) => {
                exchange.retire_connection(CloseReason::ProtocolClosed);
            }
            Poll::Pending => {
                let mut readiness = H1ReadinessTask::new(exchange);
                self.spawner.spawn(Box::pin(async move {
                    match poll_fn(|cx| readiness.poll_ready(cx)).await {
                        Ok(()) => readiness.reuse(),
                        Err(_) => readiness.retire_connection(CloseReason::ProtocolClosed),
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
    /// Arms owner-runtime fallback around one pending exchange.
    fn new(exchange: H1Exchange) -> Self {
        Self {
            exchange: Some(exchange),
        }
    }

    /// Polls Hyper for proof that another request may be sent.
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), hyper::Error>> {
        self.exchange
            .as_mut()
            .expect("HTTP/1 readiness task consumed more than once")
            .poll_ready(cx)
    }

    /// Returns the ready handle through ordinary owning-cell arbitration.
    fn reuse(mut self) {
        let exchange = self
            .exchange
            .take()
            .expect("HTTP/1 readiness task consumed more than once");
        exchange.offer_for_reuse();
    }

    /// Retires the connection after a terminal readiness result.
    fn retire_connection(mut self, reason: CloseReason) {
        self.exchange
            .take()
            .expect("HTTP/1 readiness task consumed more than once")
            .retire_connection(reason);
    }
}

impl Drop for H1ReadinessTask {
    fn drop(&mut self) {
        if let Some(exchange) = self.exchange.take() {
            exchange.retire_connection(CloseReason::OwnerRuntimeShutdown);
        }
    }
}

/// Adds the mandatory HTTP/1 Host header from the absolute request URI.
fn add_host_header(
    request: &mut Request<SdkBody>,
    absolute_uri: &Uri,
) -> Result<(), RequestDispatchError> {
    if request.headers().contains_key(http_1x::header::HOST) {
        return Ok(());
    }
    let authority = absolute_uri
        .authority()
        .ok_or(RequestDispatchError::MissingAuthority)?;
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
    let value =
        http_1x::HeaderValue::from_str(&host).map_err(RequestDispatchError::InvalidHostHeader)?;
    request.headers_mut().insert(http_1x::header::HOST, value);
    Ok(())
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
    use crate::client::pool::registry::PartitionState;
    use crate::client::pool::{
        Client, ConnectionId, ConnectionPool, ConnectionReuseScope, Partition, PartitionId,
        TokioDriverSpawner,
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

        drop(H1ReadinessTask::new(selection.into_exchange()));

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

    #[test]
    fn host_header_preserves_or_derives_the_authority() {
        let cases = [
            (
                "existing",
                "http://ignored.example/",
                Some("kept.example"),
                "kept.example",
            ),
            (
                "implicit default port",
                "http://example.com/",
                None,
                "example.com",
            ),
            (
                "HTTP default port",
                "http://example.com:80/",
                None,
                "example.com",
            ),
            (
                "HTTPS default port",
                "https://example.com:443/",
                None,
                "example.com",
            ),
            (
                "explicit port",
                "http://example.com:8080/",
                None,
                "example.com:8080",
            ),
            ("IPv6 default port", "http://[::1]:80/", None, "[::1]"),
            (
                "IPv6 explicit port",
                "http://[::1]:8080/",
                None,
                "[::1]:8080",
            ),
        ];

        for (name, uri, existing, expected) in cases {
            let absolute_uri: Uri = uri.parse().unwrap();
            let mut request = Request::get(absolute_uri.clone())
                .body(SdkBody::empty())
                .unwrap();
            if let Some(existing) = existing {
                request
                    .headers_mut()
                    .insert(http_1x::header::HOST, existing.parse().unwrap());
            }

            add_host_header(&mut request, &absolute_uri).unwrap();

            assert_eq!(
                expected,
                request.headers()[http_1x::header::HOST].to_str().unwrap(),
                "{name}"
            );
        }
    }

    #[test]
    fn h1_request_target_uses_the_required_wire_form() {
        let cases = [
            (
                "CONNECT",
                Method::CONNECT,
                "http://example.com:8443/ignored",
                false,
                "example.com:8443",
            ),
            (
                "direct",
                Method::GET,
                "http://example.com/path?key=value",
                false,
                "/path?key=value",
            ),
            (
                "proxy",
                Method::GET,
                "http://example.com/path?key=value",
                true,
                "http://example.com/path?key=value",
            ),
        ];

        for (name, method, uri, is_proxied, expected) in cases {
            let mut request = Request::builder()
                .method(method)
                .uri(uri)
                .body(SdkBody::empty())
                .unwrap();

            rewrite_h1_request_target(&mut request, is_proxied);

            assert_eq!(expected, request.uri().to_string(), "{name}");
        }
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
        let pool = super::super::super::builder::Builder::default()
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
        let exchange = selection.into_exchange();
        assert!(exchange.is_ready());

        H1ResponseLifecycle {
            exchange: Some(exchange),
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
        let pool = super::super::super::builder::Builder::default()
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
        assert_eq!(Some(PartitionId::from_index(8)), error.partition());
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
        let spawner = partition.owner_spawner().unwrap();
        partition.ensure_maintenance_started(spawner.as_ref());
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

    #[cfg(any(
        target_os = "android",
        target_os = "fuchsia",
        target_os = "illumos",
        target_os = "ios",
        target_os = "linux",
        target_os = "macos",
        target_os = "solaris",
        target_os = "tvos",
        target_os = "visionos",
        target_os = "watchos",
    ))]
    #[tokio::test]
    async fn network_interface_scope_reuses_matching_and_reclaims_mismatched_h1() {
        let server = TestServer::start().await;
        let first_id = PartitionId::from_index(1);
        let matching_id = PartitionId::from_index(2);
        let mismatched_id = PartitionId::from_index(3);
        let pool = super::super::super::builder::Builder::default()
            .idle_timeout(None)
            .partitions([
                Partition::new(first_id, TokioDriverSpawner::current())
                    .interface("synthetic-interface-a"),
                Partition::new(matching_id, TokioDriverSpawner::current())
                    .interface("synthetic-interface-a"),
                Partition::new(mismatched_id, TokioDriverSpawner::current())
                    .interface("synthetic-interface-b"),
            ])
            .connection_reuse_scope(ConnectionReuseScope::NetworkInterface)
            .max_connections_per_host(1)
            // Injected connectors deliberately ignore OS interface placement.
            .build_with_connector(ExtraConnector)
            .unwrap();
        let first = pool.inner.registry.partition(first_id).unwrap();
        let matching = pool.inner.registry.partition(matching_id).unwrap();
        let mismatched = pool.inner.registry.partition(mismatched_id).unwrap();
        let first_uri = server.uri("/first-interface");

        consume(request(&pool, first.clone(), first_uri.clone()).await).await;
        let first_connection = pool
            .inner
            .registry
            .resolve_cell(&first, &first_uri)
            .unwrap()
            .only_h1_connection_for_test();
        consume(request(&pool, matching, server.uri("/matching-interface")).await).await;
        assert_eq!(
            1,
            server.accepted(),
            "matching interface groups did not share the existing connection"
        );

        consume(request(&pool, mismatched, server.uri("/mismatched-interface")).await).await;
        assert_eq!(
            Some(CloseReason::Reclaimed),
            first_connection.snapshot().close_reason
        );
        assert_eq!(
            2,
            server.accepted(),
            "mismatched interface groups did not reclaim bounded capacity"
        );
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
        let pool = super::super::super::builder::Builder::default()
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
        let pool = super::super::super::builder::Builder::default()
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
