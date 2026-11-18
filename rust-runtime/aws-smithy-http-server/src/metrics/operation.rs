/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::task::{Context, Poll};

use tower::Service;

use crate::{operation::OperationShape, shape_id::ShapeId};

use super::state::MetricsState;

/// Records which operation a request was routed to, then calls the operation's route.
///
/// Generated services wrap each operation's route, outside its HTTP plugins, in a
/// `RecordOperation`. Not part of the public API.
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct RecordOperation<S> {
    inner: S,
    operation: ShapeId,
}

impl<S> RecordOperation<S> {
    #[doc(hidden)]
    pub fn new<Op: OperationShape>(inner: S) -> Self {
        Self {
            inner,
            operation: Op::ID,
        }
    }
}

impl<S, B> Service<http::Request<B>> for RecordOperation<S>
where
    S: Service<http::Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        if let Some(state) = req.extensions().get::<MetricsState>() {
            state.record_operation(&self.operation);
            #[cfg(feature = "request-id")]
            if let Some(request_id) = req.extensions().get::<crate::request::request_id::ServerRequestId>() {
                state.record_request_id(request_id);
            }
        }
        self.inner.call(req)
    }
}
