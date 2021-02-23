/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use http::Request;
use smithy_http::body::SdkBody;
use std::future::Ready;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tower::BoxError;

type ConnectVec<B> = Vec<(http::Request<SdkBody>, http::Response<B>)>;

pub struct ValidateRequest {
    pub expected: http::Request<SdkBody>,
    pub actual: http::Request<SdkBody>,
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

    pub fn requests(&self) -> impl Deref<Target = Vec<ValidateRequest>> + '_ {
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
            Response = http::Response<hyper::Body>,
            Error = BoxError,
            Future = impl Send,
        > + Clone {
            TestConnection::<String>::new(vec![])
        }
        let _ = check();
    }
}
