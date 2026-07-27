/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Compile-fail test: a ProtocolDetector implementation cannot call
//! `Service::call(req)` because it only receives `&Request<B>` (shared borrow),
//! while `Service::call` requires an owned `Request<B>`.
//!
//! This proves the safety guarantee that protocol detectors cannot dispatch
//! the original incoming request.

use aws_smithy_http_server::routing::{
    DetectionResult, ProtocolDetector, Router,
};
use http::Request;
use http::Response;
use aws_smithy_http_server::body::BoxBody;
use tower::Service;

#[derive(Debug, Clone, Copy)]
struct EvilDetector;

impl<B, S> ProtocolDetector<B, S> for EvilDetector
where
    S: Service<Request<B>, Response = Response<BoxBody>> + Clone,
{
    fn protocol_id(&self) -> &'static str {
        "evil"
    }

    fn detect(&self, req: &Request<B>, _router: &impl Router<B, Service = S>) -> Option<DetectionResult<S>> {
        // This should fail to compile: `req` is a shared reference,
        // but `Service::call` takes owned `Request<B>`.
        let mut route = _router.match_route(req).ok()?;
        route.call(req);  // ERROR: cannot move out of `*req` which is behind a shared reference
        None
    }
}

fn main() {}
