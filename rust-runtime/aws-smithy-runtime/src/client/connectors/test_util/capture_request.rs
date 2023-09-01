/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http::body::SdkBody;
use aws_smithy_runtime_api::client::orchestrator::HttpRequest;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// Test Connection to capture a single request
#[derive(Debug, Clone)]
pub struct CaptureRequestHandler(Arc<Mutex<Inner>>);

#[derive(Debug)]
struct Inner {
    _response: Option<http::Response<SdkBody>>,
    _sender: Option<oneshot::Sender<HttpRequest>>,
}

/// Receiver for [`CaptureRequestHandler`](CaptureRequestHandler)
#[derive(Debug)]
pub struct CaptureRequestReceiver {
    receiver: oneshot::Receiver<HttpRequest>,
}

impl CaptureRequestReceiver {
    /// Expect that a request was sent. Returns the captured request.
    ///
    /// # Panics
    /// If no request was received
    #[track_caller]
    pub fn expect_request(mut self) -> HttpRequest {
        self.receiver.try_recv().expect("no request was received")
    }

    /// Expect that no request was captured. Panics if a request was received.
    ///
    /// # Panics
    /// If a request was received
    #[track_caller]
    pub fn expect_no_request(mut self) {
        self.receiver
            .try_recv()
            .expect_err("expected no request to be received!");
    }
}

/// Test connection used to capture a single request
///
/// If response is `None`, it will reply with a 200 response with an empty body
///
/// Example:
/// ```compile_fail
/// let (server, request) = capture_request(None);
/// let conf = aws_sdk_sts::Config::builder()
///     .http_connector(server)
///     .build();
/// let client = aws_sdk_sts::Client::from_conf(conf);
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
            _response: Some(response.unwrap_or_else(|| {
                http::Response::builder()
                    .status(200)
                    .body(SdkBody::empty())
                    .expect("unreachable")
            })),
            _sender: Some(tx),
        }))),
        CaptureRequestReceiver { receiver: rx },
    )
}
