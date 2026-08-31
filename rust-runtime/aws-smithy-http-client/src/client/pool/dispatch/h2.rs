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
pub(super) enum H2DispatchResult {
    /// Hyper accepted the request and produced a guarded response.
    Response(Response<SdkBody>),
    /// Hyper returned the request envelope before accepting it.
    Reacquire(Request<SdkBody>),
}

/// State retained after Hyper accepts one request.
struct H2AcceptedDispatch {
    connection: Arc<ConnectionState>,
    close: H2CloseHandle,
    captured_metadata: Option<aws_smithy_runtime_api::client::connection::ConnectionMetadata>,
    receive_endpoint: H2LeaseEndpoint,
    reused: bool,
}

impl ConnectionPool {
    /// Dispatches one request through a prospective H2 generation lease.
    ///
    /// A request returned by a reused generation may reacquire. Failure on a
    /// fresh generation is terminal for this pool attempt.
    pub(super) async fn dispatch_h2(
        &self,
        mut request: Request<SdkBody>,
        mut activation: H2Activation,
    ) -> Result<H2DispatchResult, ConnectorError> {
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
                ConnectorError::other(H2ConnectionClosedBeforeDispatch.into(), None)
                    .with_connection(metadata),
            );
        };

        let body = H2RequestBodyHandle::arm(&mut request, send_endpoint);

        let mut send = Box::pin(sender.hyper_mut().try_send_request(request));
        let first = poll_fn(|cx| {
            Poll::Ready(match send.as_mut().poll(cx) {
                Poll::Ready(result) => FirstPoll::Ready(result),
                Poll::Pending => FirstPoll::Pending,
            })
        })
        .await;

        match first {
            FirstPoll::Ready(Err(mut error)) if error.message().is_some() => {
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
                    super::super::super::downcast_error(Box::new(error.into_error()))
                        .with_connection(metadata),
                )
            }
            FirstPoll::Ready(result) => {
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
            FirstPoll::Pending => {
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

/// Reacquires only when a pooled generation returned the request unaccepted.
#[allow(
    clippy::result_large_err,
    reason = "ConnectorError preserves SDK classification and connection metadata"
)]
fn finish_unaccepted_request(
    request: Request<SdkBody>,
    reused: bool,
    error: ConnectorError,
) -> Result<H2DispatchResult, ConnectorError> {
    if reused {
        Ok(H2DispatchResult::Reacquire(request))
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
            complete_send_endpoint(&handle.slot);
        }
        handle
    }

    /// Disarms an endpoint after Hyper returns the request unaccepted.
    fn clear(&self) {
        clear_send_endpoint(&self.slot);
    }
}

enum FirstPoll<T> {
    Ready(T),
    Pending,
}

/// Request body whose endpoint can be re-armed after certified non-acceptance.
struct H2RequestBody {
    inner: SdkBody,
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
            complete_send_endpoint(&self.slot);
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
        complete_send_endpoint(&self.slot);
    }
}

/// Response body that owns the receive endpoint through stream completion.
struct H2ResponseBody {
    inner: hyper::body::Incoming,
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

fn clear_send_endpoint(slot: &Arc<Mutex<Option<H2LeaseEndpoint>>>) {
    let endpoint = slot.lock().take();
    drop(endpoint);
}

fn complete_send_endpoint(slot: &Arc<Mutex<Option<H2LeaseEndpoint>>>) {
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
    fn bodyless_response_completes_its_receive_endpoint_without_drop() {
        let cell = cell();
        let (endpoint, probe) = H2LeaseEndpoint::receive_for_test(&cell);

        let retained = retain_receive_endpoint(true, endpoint);

        assert!(retained.is_none());
        assert!(probe.receive_complete());
    }

    #[test]
    fn only_reused_generation_reacquires_an_unaccepted_request() {
        let fresh = finish_unaccepted_request(
            Request::new(SdkBody::empty()),
            false,
            ConnectorError::user("fresh failure".into()),
        );
        assert!(fresh.is_err());

        let reused = finish_unaccepted_request(
            Request::new(SdkBody::empty()),
            true,
            ConnectorError::user("reused failure".into()),
        );
        assert!(matches!(reused, Ok(H2DispatchResult::Reacquire(_))));
    }
}
