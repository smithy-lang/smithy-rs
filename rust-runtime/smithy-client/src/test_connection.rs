/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */
//! Module with client connectors useful for testing.

// TODO
#![allow(missing_docs)]

use crate::test_connection::stream::EmptyStream;
use http::header::{HeaderName, CONTENT_TYPE};
use http::{Request, Uri};
use hyper::client::HttpConnector;
use protocol_test_helpers::{assert_ok, validate_body, MediaType};
use smithy_async::future::never::Never;
use smithy_http::body::SdkBody;
use std::future::{Future, Ready};
use std::marker::PhantomData;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tower::BoxError;

/// Test Connection to capture a single request
#[derive(Debug, Clone)]
pub struct CaptureRequestHandler(Arc<Mutex<Inner>>);

#[derive(Debug)]
struct Inner {
    response: Option<http::Response<SdkBody>>,
    sender: Option<oneshot::Sender<http::Request<SdkBody>>>,
}

/// Receiver for [`CaptureRequestHandler`](CaptureRequestHandler)
#[derive(Debug)]
pub struct CaptureRequestReceiver {
    receiver: oneshot::Receiver<http::Request<SdkBody>>,
}

impl CaptureRequestReceiver {
    pub fn expect_request(mut self) -> http::Request<SdkBody> {
        self.receiver.try_recv().expect("no request was received")
    }
}

/// A service that will never return whatever it is you want
///
/// Returned futures will return Pending forever
#[non_exhaustive]
#[derive(Debug)]
pub struct NeverService<R> {
    _resp: PhantomData<R>,
}

impl<R> Clone for NeverService<R> {
    fn clone(&self) -> Self {
        Self {
            _resp: Default::default(),
        }
    }
}

impl<R> NeverService<R> {
    pub fn new() -> Self {
        NeverService {
            _resp: Default::default(),
        }
    }
}

pub mod stream {
    use hyper::client::connect::{Connected, Connection};
    use std::io::Error;
    use std::pin::Pin;
    use std::sync::atomic::AtomicBool;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    /// A stream that will never return or accept any data
    #[non_exhaustive]
    pub struct EmptyStream;

    impl EmptyStream {
        pub fn new() -> Self {
            Self
        }
    }

    impl AsyncRead for EmptyStream {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Pending
        }
    }

    impl AsyncWrite for EmptyStream {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &[u8],
        ) -> Poll<Result<usize, Error>> {
            Poll::Pending
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
            Poll::Pending
        }

        fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
            Poll::Pending
        }
    }

    impl Connection for EmptyStream {
        fn connected(&self) -> Connected {
            Connected::new()
        }
    }
}

pub type NeverConnected = NeverService<TcpStream>;

/// A service that will connect but never send any data
#[derive(Clone)]
pub struct NeverReplies;
impl NeverReplies {
    pub fn new() -> Self {
        Self
    }
}

impl tower::Service<Uri> for NeverReplies {
    type Response = stream::EmptyStream;
    type Error = BoxError;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: Uri) -> Self::Future {
        std::future::ready(Ok(EmptyStream::new()))
    }
}

impl<Req, Resp> tower::Service<Req> for NeverService<Resp> {
    type Response = Resp;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _req: Req) -> Self::Future {
        Box::pin(async move {
            Never::new().await;
            unreachable!()
        })
    }
}

impl tower::Service<http::Request<SdkBody>> for CaptureRequestHandler {
    type Response = http::Response<SdkBody>;
    type Error = BoxError;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<SdkBody>) -> Self::Future {
        let mut inner = self.0.lock().unwrap();
        inner
            .sender
            .take()
            .expect("already sent")
            .send(req)
            .expect("channel not ready");
        std::future::ready(Ok(inner
            .response
            .take()
            .expect("could not handle second request")))
    }
}

/// Test connection used to capture a single request
///
/// If response is `None`, it will reply with a 200 response with an empty body
///
/// Example:
/// ```rust,compile_fail
/// let (server, request) = capture_request(None);
/// let client = aws_sdk_sts::Client::from_conf_conn(conf, server);
/// let _ = client.assume_role_with_saml().send().await;
/// // web identity should be unsigned
/// assert_eq!(
///     request.expect_request().headers().get("AUTHORIZATION"),
///     None
/// );
/// ```
pub fn capture_request(
    response: Option<http::Response<SdkBody>>,
) -> (CaptureRequestHandler, CaptureRequestReceiver) {
    let (tx, rx) = oneshot::channel();
    (
        CaptureRequestHandler(Arc::new(Mutex::new(Inner {
            response: Some(response.unwrap_or_else(|| {
                http::Response::builder()
                    .status(200)
                    .body(SdkBody::empty())
                    .expect("unreachable")
            })),
            sender: Some(tx),
        }))),
        CaptureRequestReceiver { receiver: rx },
    )
}

type ConnectVec<B> = Vec<(http::Request<SdkBody>, http::Response<B>)>;

#[derive(Debug)]
pub struct ValidateRequest {
    pub expected: http::Request<SdkBody>,
    pub actual: http::Request<SdkBody>,
}

impl ValidateRequest {
    pub fn assert_matches(&self, ignore_headers: &[HeaderName]) {
        let (actual, expected) = (&self.actual, &self.expected);
        for (name, value) in expected.headers() {
            if !ignore_headers.contains(name) {
                let actual_header = actual
                    .headers()
                    .get(name)
                    .unwrap_or_else(|| panic!("Header {:?} missing", name));
                assert_eq!(
                    actual_header.to_str().unwrap(),
                    value.to_str().unwrap(),
                    "Header mismatch for {:?}",
                    name
                );
            }
        }
        let actual_str = std::str::from_utf8(actual.body().bytes().unwrap_or(&[]));
        let expected_str = std::str::from_utf8(expected.body().bytes().unwrap_or(&[]));
        let media_type = if actual
            .headers()
            .get(CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().contains("json"))
            .unwrap_or(false)
        {
            MediaType::Json
        } else {
            MediaType::Other("unknown".to_string())
        };
        match (actual_str, expected_str) {
            (Ok(actual), Ok(expected)) => assert_ok(validate_body(actual, expected, media_type)),
            _ => assert_eq!(actual.body().bytes(), expected.body().bytes()),
        };
        assert_eq!(actual.uri(), expected.uri());
    }
}

/// TestConnection for use with a [`Client`](crate::Client).
///
/// A basic test connection. It will:
/// - Response to requests with a preloaded series of responses
/// - Record requests for future examination
///
/// The generic parameter `B` is the type of the response body.
/// For more complex use cases, see [Tower Test](https://docs.rs/tower-test/0.4.0/tower_test/)
/// Usage example:
/// ```rust
/// use smithy_client::test_connection::TestConnection;
/// use smithy_http::body::SdkBody;
/// let events = vec![(
///    http::Request::new(SdkBody::from("request body")),
///    http::Response::builder()
///        .status(200)
///        .body("response body")
///        .unwrap(),
/// )];
/// let conn = TestConnection::new(events);
/// let client = smithy_client::Client::from(conn);
/// ```
#[derive(Debug)]
pub struct TestConnection<B> {
    data: Arc<Mutex<ConnectVec<B>>>,
    requests: Arc<Mutex<Vec<ValidateRequest>>>,
}

// Need a clone impl that ignores `B`
impl<B> Clone for TestConnection<B> {
    fn clone(&self) -> Self {
        TestConnection {
            data: self.data.clone(),
            requests: self.requests.clone(),
        }
    }
}

impl<B> TestConnection<B> {
    pub fn new(mut data: ConnectVec<B>) -> Self {
        data.reverse();
        TestConnection {
            data: Arc::new(Mutex::new(data)),
            requests: Default::default(),
        }
    }

    pub fn requests(&self) -> impl Deref<Target = Vec<ValidateRequest>> + '_ {
        self.requests.lock().unwrap()
    }

    pub fn assert_requests_match(&self, ignore_headers: &[HeaderName]) {
        for req in self.requests().iter() {
            req.assert_matches(ignore_headers)
        }
        let remaining_requests = self.data.lock().unwrap().len();
        let actual_requests = self.requests().len();
        assert_eq!(
            remaining_requests, 0,
            "Expected {} additional requests ({} were made)",
            remaining_requests, actual_requests
        );
    }
}

impl<B> tower::Service<http::Request<SdkBody>> for TestConnection<B>
where
    SdkBody: From<B>,
{
    type Response = http::Response<SdkBody>;
    type Error = BoxError;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, actual: Request<SdkBody>) -> Self::Future {
        // todo: validate request
        if let Some((expected, resp)) = self.data.lock().unwrap().pop() {
            self.requests
                .lock()
                .unwrap()
                .push(ValidateRequest { expected, actual });
            std::future::ready(Ok(resp.map(SdkBody::from)))
        } else {
            std::future::ready(Err("No more data".into()))
        }
    }
}

impl<B> From<TestConnection<B>> for crate::Client<TestConnection<B>, tower::layer::util::Identity>
where
    B: Send + 'static,
    SdkBody: From<B>,
{
    fn from(tc: TestConnection<B>) -> Self {
        crate::Builder::new()
            .middleware(tower::layer::util::Identity::new())
            .connector(tc)
            .build()
    }
}

#[cfg(test)]
mod tests {
    use crate::test_connection::{capture_request, NeverService, TestConnection};
    use crate::{BoxError, Client};
    use hyper::service::Service;
    use smithy_http::body::SdkBody;

    fn is_send_sync<T: Send + Sync>(_: T) {}

    #[test]
    fn construct_test_client() {
        let test_conn = TestConnection::<String>::new(vec![]);
        let client: Client<_, _, _> = test_conn.into();
        is_send_sync(client);
    }

    fn is_valid_smithy_connector<T>(_: T)
    where
        T: Service<http::Request<SdkBody>, Response = http::Response<SdkBody>>
            + Send
            + Sync
            + Clone
            + 'static,
        T::Error: Into<BoxError> + Send + Sync + 'static,
        T::Future: Send + 'static,
    {
    }

    #[test]
    fn oneshot_client() {
        let (tx, _rx) = capture_request(None);
        is_valid_smithy_connector(tx);
    }

    #[test]
    fn never_test() {
        is_valid_smithy_connector(NeverService::new())
    }
}
