/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP/2 dispatch and two-ended request completion.
//!
//! [`H2Activation`] arrives with a prospective generation lease and a transient
//! sender cloned from the connection-owning cell. Dispatch first checks sender
//! and logical-connection state, then polls Hyper once. A request returned from
//! that poll was not accepted and may re-enter protocol acquisition.
//!
//! After Hyper accepts the request, the activation becomes an accepted lease.
//! [`H2RequestBody`] owns its send endpoint and [`H2ResponseBody`] owns its
//! receive endpoint. Either may finish first; the generation request count is
//! released only after both finish. A returned request re-arms the existing
//! body wrapper rather than nesting wrappers around the original body.

use super::super::cell::h2::{H2Activation, H2CloseHandle, H2DispatchParts, H2LeaseEndpoint};
use super::super::connection::ConnectionState;
use super::super::ConnectionPool;
use super::{AcquisitionContext, H1HostHeaderInserted};
use crate::sync::{Arc, Mutex};
use aws_smithy_runtime_api::client::connection::CaptureSmithyConnection;
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_types::body::SdkBody;
use http_1x::{Request, Response};
use hyper::body::Body;
use std::future::{poll_fn, Future};
use std::pin::Pin;
use std::task::{Context, Poll};

/// Result of one checked HTTP/2 dispatch attempt.
#[allow(
    clippy::large_enum_variant,
    reason = "boxing the successful response would allocate on every HTTP/2 request"
)]
pub(super) enum H2DispatchResult {
    /// Hyper accepted the request and produced a guarded response.
    Response(Response<SdkBody>),
    /// Hyper did not accept the request, so it may re-enter acquisition.
    Reacquire(Box<H2Reacquisition>),
}

/// Request and terminal fallback retained for one replacement selection.
pub(super) struct H2Reacquisition {
    /// Original request Hyper did not accept.
    request: Request<SdkBody>,
    /// Error returned if the request exhausts its replacement budget.
    error: ConnectorError,
}

impl H2Reacquisition {
    /// Returns the original request and its terminal fallback.
    pub(super) fn into_parts(self: Box<Self>) -> (Request<SdkBody>, ConnectorError) {
        let Self { request, error } = *self;
        (request, error)
    }
}

/// State retained after Hyper accepts one request.
struct H2AcceptedDispatch {
    /// Connection retained through response creation.
    connection: Arc<ConnectionState>,
    /// Generation close authority used for connection-wide failures.
    close: H2CloseHandle,
    /// Metadata captured before the request moved into Hyper.
    captured_metadata: Option<aws_smithy_runtime_api::client::connection::ConnectionMetadata>,
    /// Receive endpoint transferred to the guarded response body.
    receive_endpoint: H2LeaseEndpoint,
    /// Whether the generation accepted an earlier request.
    reused: bool,
}

impl ConnectionPool {
    /// Dispatches one request through a prospective H2 generation lease.
    ///
    /// Pool-side staleness reacquires before Hyper sees the request. Once
    /// Hyper returns an envelope, only a reused generation may reacquire.
    pub(super) async fn dispatch_h2(
        &self,
        context: &AcquisitionContext,
        mut request: Request<SdkBody>,
        mut activation: H2Activation,
    ) -> Result<H2DispatchResult, ConnectorError> {
        prepare_h2_request(&mut request, &context.absolute_uri);
        let connection = activation.connection().clone();
        let reused = activation.is_reused();
        let close = activation.close_handle();
        let captured_metadata = request
            .extensions()
            .get::<CaptureSmithyConnection>()
            .cloned()
            .map(|capture| {
                let metadata = connection.info().h2_metadata(close.clone());
                let captured = metadata.clone();
                capture.set_connection_retriever(move || Some(captured.clone()));
                metadata
            });

        let H2DispatchParts {
            mut sender,
            send_endpoint,
            receive_endpoint,
        } = activation.take_dispatch_parts();
        if sender.is_closed() {
            close.close(super::super::connection::CloseReason::ProtocolClosed);
            let metadata =
                captured_metadata.unwrap_or_else(|| connection.info().h2_metadata(close));
            return finish_unaccepted_request(
                request,
                reused,
                UnacceptedStage::BeforeHyper,
                ConnectorError::other(H2ConnectionClosedBeforeDispatch.into(), None)
                    .with_connection(metadata),
            );
        }
        let Some(dispatch) = ConnectionState::try_commit_dispatch(&connection) else {
            let metadata =
                captured_metadata.unwrap_or_else(|| connection.info().h2_metadata(close));
            return finish_unaccepted_request(
                request,
                reused,
                UnacceptedStage::BeforeHyper,
                ConnectorError::other(H2ConnectionClosedBeforeDispatch.into(), None)
                    .with_connection(metadata),
            );
        };

        let body = H2RequestBodyHandle::arm(&mut request, send_endpoint);

        let mut send = Box::pin(sender.hyper_mut().try_send_request(request));
        let first = poll_fn(|cx| Poll::Ready(send.as_mut().poll(cx))).await;

        match first {
            Poll::Ready(Err(mut error)) if error.message().is_some() => {
                let returned = error
                    .take_message()
                    .expect("checked returned request disappeared");
                body.clear();
                drop(receive_endpoint);
                drop(dispatch);
                drop(activation);
                close.close(super::super::connection::CloseReason::ProtocolClosed);
                let metadata =
                    captured_metadata.unwrap_or_else(|| connection.info().h2_metadata(close));
                finish_unaccepted_request(
                    returned,
                    reused,
                    UnacceptedStage::ReturnedByHyper,
                    super::super::super::downcast_error(Box::new(error.into_error()))
                        .with_connection(metadata),
                )
            }
            Poll::Ready(result) => {
                activation.accept(dispatch);
                let sender_closed = sender.is_closed();
                self.finish_h2_send(
                    sender_closed,
                    result,
                    H2AcceptedDispatch {
                        connection,
                        close,
                        captured_metadata,
                        receive_endpoint,
                        reused,
                    },
                )
            }
            Poll::Pending => {
                activation.accept(dispatch);
                let result = send.await;
                let sender_closed = sender.is_closed();
                self.finish_h2_send(
                    sender_closed,
                    result,
                    H2AcceptedDispatch {
                        connection,
                        close,
                        captured_metadata,
                        receive_endpoint,
                        reused,
                    },
                )
            }
        }
    }

    #[allow(
        clippy::result_large_err,
        reason = "ConnectorError preserves SDK classification and connection metadata"
    )]
    fn finish_h2_send(
        &self,
        sender_closed: bool,
        result: Result<
            Response<hyper::body::Incoming>,
            hyper::client::conn::TrySendError<Request<SdkBody>>,
        >,
        accepted: H2AcceptedDispatch,
    ) -> Result<H2DispatchResult, ConnectorError> {
        let H2AcceptedDispatch {
            connection,
            close,
            captured_metadata,
            receive_endpoint,
            reused,
        } = accepted;
        match result {
            Ok(mut response) => {
                connection
                    .info()
                    .apply_connector_extras(response.extensions_mut());
                let (parts, body) = response.into_parts();
                let body = H2ResponseBody::new(body, receive_endpoint);
                Ok(H2DispatchResult::Response(Response::from_parts(
                    parts,
                    SdkBody::from_body_1_x(body),
                )))
            }
            Err(mut error) => {
                // Hyper may return a queued envelope when its connection task
                // drops after the first poll. The returned request is the
                // authority for reacquisition; an error without it is terminal.
                if let Some(request) = error.take_message() {
                    if let Some(body) = request.extensions().get::<H2RequestBodyHandle>() {
                        body.clear();
                    }
                    drop(receive_endpoint);
                    close.close(super::super::connection::CloseReason::ProtocolClosed);
                    let metadata =
                        captured_metadata.unwrap_or_else(|| connection.info().h2_metadata(close));
                    return finish_unaccepted_request(
                        request,
                        reused,
                        UnacceptedStage::ReturnedByHyper,
                        super::super::super::downcast_error(Box::new(error.into_error()))
                            .with_connection(metadata),
                    );
                }
                if sender_closed {
                    close.close(super::super::connection::CloseReason::ProtocolClosed);
                }
                drop(receive_endpoint);
                let metadata =
                    captured_metadata.unwrap_or_else(|| connection.info().h2_metadata(close));
                Err(
                    super::super::super::downcast_error(Box::new(error.into_error()))
                        .with_connection(metadata),
                )
            }
        }
    }
}

/// Restores the absolute URI and removes only a pool-synthesized H1 `Host`.
fn prepare_h2_request(request: &mut Request<SdkBody>, absolute_uri: &http_1x::Uri) {
    *request.uri_mut() = absolute_uri.clone();
    if request
        .extensions_mut()
        .remove::<H1HostHeaderInserted>()
        .is_some()
    {
        request.headers_mut().remove(http_1x::header::HOST);
    }
}

/// Point at which the pool learned that Hyper had not accepted the request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UnacceptedStage {
    /// The pool rejected a stale generation before calling Hyper.
    BeforeHyper,
    /// Hyper returned the original request envelope.
    ReturnedByHyper,
}

/// Applies retry authority to a request that Hyper did not accept.
///
/// Pool-side checks always reacquire because no protocol code observed the
/// request. A returned Hyper envelope reacquires only after prior successful
/// use proves that replacing a stale pooled generation is appropriate.
#[allow(
    clippy::result_large_err,
    reason = "ConnectorError preserves SDK classification and connection metadata"
)]
fn finish_unaccepted_request(
    request: Request<SdkBody>,
    reused: bool,
    stage: UnacceptedStage,
    error: ConnectorError,
) -> Result<H2DispatchResult, ConnectorError> {
    if stage == UnacceptedStage::BeforeHyper || reused {
        Ok(H2DispatchResult::Reacquire(Box::new(H2Reacquisition {
            request,
            error,
        })))
    } else {
        Err(error)
    }
}

/// A fresh HTTP/2 generation closed before accepting its first request.
#[derive(Debug)]
struct H2ConnectionClosedBeforeDispatch;

impl std::fmt::Display for H2ConnectionClosedBeforeDispatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the fresh HTTP/2 connection closed before request dispatch")
    }
}

impl std::error::Error for H2ConnectionClosedBeforeDispatch {}

/// Handle retained in a request extension after its body is wrapped once.
#[derive(Clone)]
struct H2RequestBodyHandle {
    /// Re-armable send endpoint shared with the wrapped request body.
    slot: Arc<Mutex<Option<H2LeaseEndpoint>>>,
}

impl H2RequestBodyHandle {
    /// Wraps an unwrapped body and arms the current send endpoint.
    fn arm(request: &mut Request<SdkBody>, endpoint: H2LeaseEndpoint) -> Self {
        let is_end_stream = request.body().is_end_stream();
        let handle = request
            .extensions()
            .get::<Self>()
            .cloned()
            .unwrap_or_else(|| {
                let handle = Self {
                    slot: Arc::new(Mutex::new(None)),
                };
                let body = std::mem::replace(request.body_mut(), SdkBody::taken());
                *request.body_mut() = SdkBody::from_body_1_x(H2RequestBody {
                    inner: body,
                    slot: handle.slot.clone(),
                });
                request.extensions_mut().insert(handle.clone());
                handle
            });
        arm_send_endpoint(&handle.slot, endpoint);
        if is_end_stream {
            finish_send_endpoint(&handle.slot);
        }
        handle
    }

    /// Disarms an endpoint after Hyper returns the request unaccepted.
    fn clear(&self) {
        finish_send_endpoint(&self.slot);
    }
}

/// Request body whose endpoint can be re-armed after certified non-acceptance.
struct H2RequestBody {
    /// Original SDK body wrapped exactly once.
    inner: SdkBody,
    /// Endpoint replaced when Hyper returns an unaccepted request.
    slot: Arc<Mutex<Option<H2LeaseEndpoint>>>,
}

impl Body for H2RequestBody {
    type Data = <SdkBody as Body>::Data;
    type Error = <SdkBody as Body>::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
        let result = Pin::new(&mut self.inner).poll_frame(cx);
        if matches!(result, Poll::Ready(None) | Poll::Ready(Some(Err(_)))) {
            finish_send_endpoint(&self.slot);
        }
        result
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        self.inner.size_hint()
    }
}

impl Drop for H2RequestBody {
    fn drop(&mut self) {
        finish_send_endpoint(&self.slot);
    }
}

/// Response body that owns the receive endpoint through stream completion.
struct H2ResponseBody {
    /// Hyper response stream.
    inner: hyper::body::Incoming,
    /// Receive endpoint completed on terminal frame, error, or drop.
    receive: Option<H2LeaseEndpoint>,
}

impl H2ResponseBody {
    /// Wraps a response and completes an endpoint already at end stream.
    fn new(inner: hyper::body::Incoming, receive: H2LeaseEndpoint) -> Self {
        let is_end_stream = inner.is_end_stream();
        Self {
            inner,
            receive: retain_receive_endpoint(is_end_stream, receive),
        }
    }
}

/// Retains a receive endpoint only while response frames may remain.
fn retain_receive_endpoint(
    is_end_stream: bool,
    receive: H2LeaseEndpoint,
) -> Option<H2LeaseEndpoint> {
    if is_end_stream {
        receive.complete();
        None
    } else {
        Some(receive)
    }
}

impl Body for H2ResponseBody {
    type Data = <hyper::body::Incoming as Body>::Data;
    type Error = <hyper::body::Incoming as Body>::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
        let result = Pin::new(&mut self.inner).poll_frame(cx);
        if matches!(result, Poll::Ready(None) | Poll::Ready(Some(Err(_)))) {
            if let Some(endpoint) = self.receive.take() {
                endpoint.complete();
            }
        }
        result
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        self.inner.size_hint()
    }
}

fn arm_send_endpoint(slot: &Arc<Mutex<Option<H2LeaseEndpoint>>>, endpoint: H2LeaseEndpoint) {
    let previous = slot.lock().replace(endpoint);
    drop(previous);
}

/// Terminates the send endpoint after upload completion, error, or rejection.
fn finish_send_endpoint(slot: &Arc<Mutex<Option<H2LeaseEndpoint>>>) {
    let endpoint = slot.lock().take();
    if let Some(endpoint) = endpoint {
        endpoint.complete();
    }
}

#[cfg(all(test, not(smithy_http_client_loom), feature = "rt-tokio"))]
mod tests {
    use super::*;
    use crate::client::pool::cell::OriginCell;
    use crate::client::pool::origin::OriginKey;
    use crate::client::pool::partition::{EligibilityGroup, PartitionId};
    use crate::sync::Arc;
    use http_1x::uri::Scheme;
    use http_body_util::BodyExt;

    /// Request body that fails on its first frame.
    struct FailingBody;

    impl Body for FailingBody {
        type Data = hyper::body::Bytes;
        type Error = std::io::Error;

        fn poll_frame(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
            Poll::Ready(Some(Err(std::io::Error::other("request body failed"))))
        }
    }

    fn cell() -> Arc<OriginCell> {
        Arc::new(OriginCell::new(
            PartitionId::from_index(1),
            OriginKey::from_parts(Scheme::HTTPS, "example.com", None).unwrap(),
            EligibilityGroup::Pool,
            None,
            None,
        ))
    }

    #[tokio::test]
    async fn returned_request_rearms_one_body_wrapper() {
        let cell = cell();
        let mut request = Request::post("https://example.com/upload")
            .body(SdkBody::from("payload"))
            .unwrap();

        let (first_endpoint, first_probe) = H2LeaseEndpoint::send_for_test(&cell);
        let first = H2RequestBodyHandle::arm(&mut request, first_endpoint);
        let first_slot = first.slot.clone();
        assert!(!first_probe.send_complete());

        let (second_endpoint, second_probe) = H2LeaseEndpoint::send_for_test(&cell);
        let second = H2RequestBodyHandle::arm(&mut request, second_endpoint);
        assert!(
            Arc::ptr_eq(&first_slot, &second.slot),
            "rearming replaced the request-body wrapper"
        );
        assert!(
            first_probe.send_complete(),
            "rearming did not cancel the prior prospective endpoint"
        );
        assert!(!second_probe.send_complete());

        second.clear();
        assert!(
            second_probe.send_complete(),
            "returned request retained its rejected send endpoint"
        );
        let body = request
            .into_body()
            .collect()
            .await
            .expect("wrapped request body failed")
            .to_bytes();
        assert_eq!(hyper::body::Bytes::from_static(b"payload"), body);
    }

    #[test]
    fn empty_request_body_completes_its_send_endpoint_when_armed() {
        let cell = cell();
        let mut request = Request::new(SdkBody::empty());
        let (endpoint, probe) = H2LeaseEndpoint::send_for_test(&cell);

        H2RequestBodyHandle::arm(&mut request, endpoint);

        assert!(probe.send_complete());
    }
    #[test]
    fn dropping_request_body_completes_its_send_endpoint() {
        let cell = cell();
        let mut request = Request::new(SdkBody::from("payload"));
        let (endpoint, probe) = H2LeaseEndpoint::send_for_test(&cell);

        H2RequestBodyHandle::arm(&mut request, endpoint);
        let body = std::mem::replace(request.body_mut(), SdkBody::empty());
        drop(body);

        assert!(probe.send_complete());
    }

    #[tokio::test]
    async fn request_body_error_completes_its_send_endpoint() {
        let cell = cell();
        let mut request = Request::new(SdkBody::from_body_1_x(FailingBody));
        let (endpoint, probe) = H2LeaseEndpoint::send_for_test(&cell);

        H2RequestBodyHandle::arm(&mut request, endpoint);
        let frame = request
            .body_mut()
            .frame()
            .await
            .expect("failing body omitted its error frame");

        assert!(frame.is_err());
        assert!(probe.send_complete());
    }

    #[test]
    fn bodyless_response_completes_its_receive_endpoint_without_drop() {
        let cell = cell();
        let (endpoint, probe) = H2LeaseEndpoint::receive_for_test(&cell);

        let retained = retain_receive_endpoint(true, endpoint);

        assert!(retained.is_none());
        assert!(probe.receive_complete());
    }

    #[test]
    fn h2_preparation_restores_uri_and_removes_only_synthesized_host() {
        let absolute: http_1x::Uri = "https://example.com/resource".parse().unwrap();
        let mut request = Request::get("/resource").body(SdkBody::empty()).unwrap();
        request
            .headers_mut()
            .insert(http_1x::header::HOST, "example.com".parse().unwrap());
        request.extensions_mut().insert(H1HostHeaderInserted);

        prepare_h2_request(&mut request, &absolute);

        assert_eq!(&absolute, request.uri());
        assert!(!request.headers().contains_key(http_1x::header::HOST));
        assert!(request.extensions().get::<H1HostHeaderInserted>().is_none());

        let mut user_host = Request::get("/resource").body(SdkBody::empty()).unwrap();
        user_host
            .headers_mut()
            .insert(http_1x::header::HOST, "signed.example".parse().unwrap());
        prepare_h2_request(&mut user_host, &absolute);
        assert_eq!("signed.example", user_host.headers()[http_1x::header::HOST]);
    }

    #[test]
    fn retry_authority_distinguishes_pool_checks_from_hyper_returns() {
        let checked_before_hyper = finish_unaccepted_request(
            Request::new(SdkBody::empty()),
            false,
            UnacceptedStage::BeforeHyper,
            ConnectorError::user("pre-Hyper failure".into()),
        );
        assert!(matches!(
            checked_before_hyper,
            Ok(H2DispatchResult::Reacquire(_))
        ));

        let fresh_hyper_return = finish_unaccepted_request(
            Request::new(SdkBody::empty()),
            false,
            UnacceptedStage::ReturnedByHyper,
            ConnectorError::user("fresh Hyper failure".into()),
        );
        assert!(fresh_hyper_return.is_err());

        let reused_hyper_return = finish_unaccepted_request(
            Request::new(SdkBody::empty()),
            true,
            UnacceptedStage::ReturnedByHyper,
            ConnectorError::user("reused failure".into()),
        );
        assert!(matches!(
            reused_hyper_return,
            Ok(H2DispatchResult::Reacquire(_))
        ));
    }
}
