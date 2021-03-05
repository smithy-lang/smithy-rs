/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use http::Request;
use smithy_http::body::SdkBody;
use std::future::{Future, Ready};
use std::ops::Deref;
use std::pin::Pin;
use std::process::Output;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tower::{BoxError, Service};

type ConnectVec<B> = Vec<(http::Request<SdkBody>, http::Response<B>)>;

pub struct ValidateRequest {
    pub expected: http::Request<SdkBody>,
    pub actual: http::Request<SdkBody>,
}

impl ValidateRequest {
    pub fn assert_matches(&self, ignore_headers: Vec<HeaderName>) {
        let (actual, expected) = (&self.actual, &self.expected);
        for (name, value) in expected.headers() {
            if !ignore_headers.contains(name) {
                let actual_header = actual.headers().get(name).expect(&format!("Header {:?} missing", name));
                assert_eq!(actual_header, value, "Header mismatch for {:?}", name);
            }
        }
        let actual_str = std::str::from_utf8(actual.body().bytes().unwrap_or(&[]));
        let expected_str = std::str::from_utf8(expected.body().bytes().unwrap_or(&[]));
        match (actual_str, expected_str) {
            (Ok(actual), Ok(expected)) => assert_eq!(actual, expected),
            _ => assert_eq!(actual.body().bytes(), expected.body().bytes())
        };
        assert_eq!(actual.uri(), expected.uri());
    }
}

/// TestConnection for use with a [`aws_hyper::Client`](crate::Client)
///
/// A basic test connection. It will:
/// - Response to requests with a preloaded series of responses
/// - Record requests for future examination
///
/// For more complex use cases, see [Tower Test](https://docs.rs/tower-test/0.4.0/tower_test/)
/// Usage example:
/// ```rust
/// use aws_hyper::test_connection::TestConnection;
/// use smithy_http::body::SdkBody;
/// let events = vec![(
///    http::Request::new(SdkBody::from("request body")),
///    http::Response::builder()
///        .status(200)
///        .body("response body")
///        .unwrap(),
/// )];
/// let conn = TestConnection::new(events);
/// let client = aws_hyper::Client::new(conn);
/// ```
#[derive(Clone)]
pub struct TestConnection<B> {
    data: Arc<Mutex<ConnectVec<B>>>,
    requests: Arc<Mutex<Vec<ValidateRequest>>>,
}

impl<B> TestConnection<B> {
    pub fn new(mut data: ConnectVec<B>) -> Self {
        data.reverse();
        TestConnection {
            data: Arc::new(Mutex::new(data)),
            requests: Default::default(),
        }
    }

    pub fn requests(&self) -> impl Deref<Target=Vec<ValidateRequest>> + '_ {
        self.requests.lock().unwrap()
    }
}

impl<B: Into<hyper::Body>> tower::Service<http::Request<SdkBody>> for TestConnection<B> {
    type Response = http::Response<hyper::Body>;
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
                .push(ValidateRequest { actual, expected });
            std::future::ready(Ok(resp.map(|body| body.into())))
        } else {
            std::future::ready(Err("No more data".into()))
        }
    }
}

#[derive(Clone)]
pub struct RecordingConnection<S> {
    pub(crate) data: Arc<Mutex<ConnectVec<Bytes>>>,
    pub(crate) inner: S,
}

use std::fmt::Write;
use bytes::Bytes;
use http::header::HeaderName;

fn write_req(req: &http::Request<SdkBody>) -> String {
    let mut o = String::new();
    write!(&mut o, "http::Request::builder()\n").unwrap();
    for (name, value) in req.headers() {
        write!(
            &mut o,
            ".header(\"{}\", \"{}\")\n",
            name.as_str(),
            value.to_str().unwrap()
        )
            .unwrap();
    }
    write!(&mut o, ".uri(Uri::from_static(\"{}\"))\n", req.uri().to_string());
    write!(&mut o, ".body(SdkBody::from(r#\"{}\"#)).unwrap()", std::str::from_utf8(req.body().bytes().unwrap()).unwrap());
    o
}

fn write_resp(resp: &http::Response<Bytes>) -> String {
    let mut o = String::new();
    write!(&mut o, "http::Response::builder()\n").unwrap();
    for (name, value) in resp.headers() {
        write!(
            &mut o,
            ".header(\"{}\", \"{}\")\n",
            name.as_str(),
            value.to_str().unwrap()
        )
            .unwrap();
    }
    write!(&mut o, ".status(http::StatusCode::from_u16({}).unwrap())\n", resp.status().as_u16());
    write!(&mut o, ".body(r#\"{}\"#).unwrap()", std::str::from_utf8(resp.body()).unwrap());
    o
}

impl<S> RecordingConnection<S> {
    pub fn dump(&self) -> String {
        let data = self.data.lock().unwrap();
        let mut o = String::new();
        write!(&mut o, "let conn = TestConnection::new(vec![").unwrap();
        for (req, resp) in data.iter() {
            /*
                   let expected_req = http::Request::builder()
            .header(USER_AGENT, "aws-sdk-rust/0.123.test os/windows/XPSP3 lang/rust/1.50.0")
            .header("x-amz-user-agent", "aws-sdk-rust/0.123.test api/test-service/0.123 os/windows/XPSP3 lang/rust/1.50.0")
            .header(HOST, "test-service.test-region.amazonaws.com")
            .header(AUTHORIZATION, "AWS4-HMAC-SHA256 Credential=access_key/20210215/test-region/test-service/aws4_request, SignedHeaders=host, Signature=b4bccc6f03b22e88b9e52a60314d4629c5d159a7cc2de25b1d687b3e5e480d2c")
            .header("x-amz-date", "20210215T184017Z")
            .uri(Uri::from_static("https://test-service.test-region.amazonaws.com/"))
            .body(SdkBody::from("request body")).unwrap();
                */
            write!(&mut o, "({}, {}),", write_req(req), write_resp(resp));
        }
        write!(&mut o, "]);");
        o
    }
}

impl<S> tower::Service<http::Request<SdkBody>> for RecordingConnection<S>
    where
        S: Service<http::Request<SdkBody>, Response=http::Response<hyper::Body>>
        + Send
        + Clone
        + 'static,
        S::Error: Into<BoxError> + Send + Sync + 'static,
        S::Future: Send + 'static,
{
    type Response = http::Response<hyper::Body>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output=Result<http::Response<hyper::Body>, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<SdkBody>) -> Self::Future {
        let cloned_body = req.body().try_clone().unwrap();
        let mut cloned_request = http::Request::builder()
            .uri(req.uri().clone())
            .method(req.method());
        *cloned_request
            .headers_mut()
            .expect("builder has not been modified, headers must be valid") = req.headers().clone();
        let cloned_request = cloned_request
            .body(cloned_body)
            .expect("a clone of a valid request should be a valid request");
        let resp_fut = self.inner.call(req);
        let data = self.data.clone();
        let fut = async move {
            let resp = resp_fut.await;
            let resp = if let Ok(mut resp) = resp {
                let resp_data = hyper::body::to_bytes(resp.body_mut()).await.unwrap();
                let mut cloned_resp = http::Response::builder()
                    .status(resp.status());
                *cloned_resp
                    .headers_mut()
                    .expect("builder has not been modified, headers must be valid") = resp.headers().clone();

                let cloned_resp = cloned_resp
                    .body(resp_data.clone())
                    .unwrap();
                data.lock().unwrap().push((cloned_request, cloned_resp));
                *(resp.body_mut()) = hyper::Body::from(resp_data.clone());
                Ok(resp)
            } else {
                resp
            };
            resp
        };
        Box::pin(fut)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_connection::TestConnection;
    use smithy_http::body::SdkBody;
    use tower::BoxError;

    /// Validate that the `TestConnection` meets the required trait bounds to be used with a aws-hyper service
    #[test]
    fn meets_trait_bounds() {
        fn check() -> impl tower::Service<
            http::Request<SdkBody>,
            Response=http::Response<hyper::Body>,
            Error=BoxError,
            Future=impl Send,
        > + Clone {
            TestConnection::<String>::new(vec![])
        }
        let _ = check();
    }
}
