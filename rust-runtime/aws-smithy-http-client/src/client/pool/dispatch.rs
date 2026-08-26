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
        let owner_spawner = handshake::owner_spawner(&partition)?;
        partition.start_maintenance(owner_spawner.as_ref());

        validate_h1_request(&request)?;
        add_host_header(&mut request, &absolute_uri)?;

        loop {
            let mut selection = self
                .acquire_h1(
                    partition.clone(),
                    cell.clone(),
                    absolute_uri.clone(),
                    connect_timeout.clone(),
                )
                .await?;
            let request_method = request.method().clone();
            let reused = selection.is_reused();
            let metadata = selection
                .connection()
                .info()
                .metadata(selection.close_handle());
            if let Some(capture) = request
                .extensions()
                .get::<CaptureSmithyConnection>()
                .cloned()
            {
                let metadata = metadata.clone();
                capture.set_connection_retriever(move || Some(metadata.clone()));
            }

            rewrite_h1_request_target(&mut request, selection.connection().info().is_proxied());
            let connection = selection.connection().clone();
            let mut request_slot = Some(request);
            let prepared = poll_fn(
                |cx| match selection.sender_mut().hyper_mut().poll_ready(cx) {
                    Poll::Ready(Ok(())) => {
                        let Some(dispatch) = ConnectionState::try_commit_dispatch(&connection)
                        else {
                            return Poll::Ready(None);
                        };
                        let request = request_slot
                            .take()
                            .expect("HTTP/1 request was dispatched more than once");
                        Poll::Ready(Some((
                            selection.sender_mut().hyper_mut().try_send_request(request),
                            dispatch,
                        )))
                    }
                    Poll::Ready(Err(_)) => Poll::Ready(None),
                    Poll::Pending => Poll::Pending,
                },
            )
            .await;

            let Some((send, dispatch)) = prepared else {
                tracing::trace!(
                    connection_id = %connection.id(),
                    "HTTP/1 selection became stale before dispatch; retrying"
                );
                request = request_slot
                    .take()
                    .expect("pre-dispatch failure lost the HTTP/1 request");
                *request.uri_mut() = absolute_uri.clone();
                selection.retire(CloseReason::ProtocolClosed);
                continue;
            };

            let return_task = selection.into_return_task();
            let response = send.await;
            match response {
                Ok(response) => {
                    return Ok(guard_response(
                        response,
                        request_method,
                        return_task,
                        dispatch,
                        handshake::owner_spawner(&partition)?,
                    ));
                }
                Err(mut error) => {
                    if let Some(mut returned) = error.take_message() {
                        tracing::trace!(
                            connection_id = %connection.id(),
                            "reused HTTP/1 connection rejected request; retrying"
                        );
                        return_task.retire(CloseReason::ProtocolClosed);
                        drop(dispatch);
                        if reused {
                            *returned.uri_mut() = absolute_uri.clone();
                            request = returned;
                            continue;
                        }
                        return Err(super::super::downcast_error(Box::new(error.into_error()))
                            .with_connection(metadata));
                    }
                    return_task.retire(CloseReason::IncompleteH1Exchange);
                    drop(dispatch);
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
                        permit,
                        connect_timeout.clone(),
                    );
                    let completion_cell = cell.clone();
                    let spawner = handshake::owner_spawner(&partition)?;
                    spawner.spawn(Box::pin(async move {
                        if !completion_cell.start_establishment(waiter) {
                            drop(attempt);
                            return;
                        }
                        let result = attempt.await;
                        completion_cell.complete_establishment(
                            waiter,
                            result
                                .map(AcquisitionResult::H1)
                                .unwrap_or_else(AcquisitionResult::Failed),
                        );
                    }));
                }
            }
        }
    }
}

/// Cancels a retained acquisition episode when its request future is dropped.
struct WaiterGuard {
    cell: Arc<OriginCell>,
    waiter: super::cell::WaiterId,
    active: bool,
}

impl WaiterGuard {
    fn new(cell: Arc<OriginCell>, waiter: super::cell::WaiterId) -> Self {
        Self {
            cell,
            waiter,
            active: true,
        }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for WaiterGuard {
    fn drop(&mut self) {
        if self.active {
            self.cell.cancel_waiter(self.waiter);
        }
    }
}

/// Wraps an accepted response with H1 return and dispatch ownership.
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
    return_task: Option<H1ReturnTask>,
    dispatch: Option<DispatchGuard>,
    spawner: StdArc<dyn DriverSpawner>,
}

impl H1ResponseLifecycle {
    fn finish(mut self, cx: Option<&mut Context<'_>>) {
        let mut return_task = self
            .return_task
            .take()
            .expect("HTTP/1 response lifecycle lost its return task");
        let ready = match cx {
            Some(cx) => return_task.poll_ready(cx),
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
                self.spawner.spawn(Box::pin(async move {
                    match poll_fn(|cx| return_task.poll_ready(cx)).await {
                        Ok(()) => {
                            if let Some(offer) = return_task.into_offer() {
                                offer.resolve();
                            }
                        }
                        Err(_) => {
                            return_task.retire(CloseReason::ProtocolClosed);
                        }
                    }
                }));
            }
        }
        if let Some(dispatch) = self.dispatch.take() {
            dispatch.complete();
        }
    }
}

/// Adds the mandatory HTTP/1 Host header from the absolute request URI.
fn add_host_header(
    request: &mut Request<SdkBody>,
    absolute_uri: &Uri,
) -> Result<(), ConnectorError> {
    if request.headers().contains_key(http_1x::header::HOST) {
        return Ok(());
    }
    let authority = absolute_uri
        .authority()
        .ok_or_else(|| ConnectorError::user("request URI must contain an authority".into()))?;
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
        .map_err(|error| ConnectorError::user(error.into()))?;
    request.headers_mut().insert(http_1x::header::HOST, value);
    Ok(())
}

/// Rejects request versions Hyper's legacy client does not dispatch on H1.
fn validate_h1_request(request: &Request<SdkBody>) -> Result<(), ConnectorError> {
    match request.version() {
        Version::HTTP_11 => Ok(()),
        Version::HTTP_10 if request.method() == Method::CONNECT => Err(ConnectorError::user(
            "CONNECT is not supported over HTTP/1.0".into(),
        )),
        Version::HTTP_10 => Ok(()),
        Version::HTTP_2 => Err(ConnectorError::user(
            "an HTTP/2 request cannot use an HTTP/1 connection".into(),
        )),
        version => Err(ConnectorError::user(
            format!("unsupported HTTP version: {version:?}").into(),
        )),
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
    use crate::client::pool::{Client, ConnectionPool, Partition, PartitionId, TokioDriverSpawner};
    use crate::client::timeout::test::{NeverConnects, NeverReplies};
    use aws_smithy_async::rt::sleep::TokioSleep;
    use aws_smithy_runtime_api::client::connection::CaptureSmithyConnection;
    use aws_smithy_runtime_api::client::http::{HttpClient, HttpConnector, HttpConnectorSettings};
    use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
    use aws_smithy_runtime_api::client::runtime_components::RuntimeComponentsBuilder;
    use http_body_util::BodyExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

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

        consume(request(&pool, partition, server.uri("/second")).await).await;
        assert_eq!(2, server.accepted());
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

    #[tokio::test]
    async fn explicit_partition_drives_h1_on_its_owner_runtime() {
        let server = TestServer::start().await;
        let owner = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let pool = ConnectionPool::builder()
            .partitions([Partition::new(
                PartitionId::from_index(7),
                TokioDriverSpawner::from_handle(owner.handle().clone()),
            )])
            .build_http()
            .unwrap();
        let partition = pool
            .inner
            .registry
            .partition(PartitionId::from_index(7))
            .unwrap();

        assert_eq!(
            hyper::body::Bytes::from_static(b"ok"),
            consume(request(&pool, partition, server.uri("/explicit")).await).await
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

    #[tokio::test]
    async fn h1_upgrade_transfers_io_and_never_returns_to_the_pool() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let accepted = StdArc::new(AtomicUsize::new(0));
        let accepted_for_task = accepted.clone();
        let server = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let index = accepted_for_task.fetch_add(1, Ordering::SeqCst);
                if index == 0 {
                    tokio::spawn(async move {
                        if !read_request_head(&mut stream).await {
                            return;
                        }
                        stream
                            .write_all(
                                b"HTTP/1.1 101 Switching Protocols\r\n\
                                  connection: upgrade\r\n\
                                  upgrade: test\r\n\r\nhello",
                            )
                            .await
                            .unwrap();
                        let mut sink = [0_u8; 32];
                        while stream.read(&mut sink).await.unwrap_or(0) != 0 {}
                    });
                } else {
                    tokio::spawn(serve_connection(stream));
                }
            }
        });

        let pool = ConnectionPool::builder().build_http().unwrap();
        let partition = anonymous_partition(&pool);
        let upgrade_request = Request::get(format!("{endpoint}/upgrade"))
            .header(http_1x::header::CONNECTION, "upgrade")
            .header(http_1x::header::UPGRADE, "test")
            .body(SdkBody::empty())
            .unwrap();
        let mut response = pool
            .send_request(partition.clone(), upgrade_request, None)
            .await
            .unwrap();
        assert_eq!(http_1x::StatusCode::SWITCHING_PROTOCOLS, response.status());

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
        drop(upgraded);
        drop(response);

        let second_uri: Uri = format!("{endpoint}/after").parse().unwrap();
        consume(request(&pool, partition, second_uri).await).await;
        assert_eq!(2, accepted.load(Ordering::SeqCst));
        server.abort();
    }
}
