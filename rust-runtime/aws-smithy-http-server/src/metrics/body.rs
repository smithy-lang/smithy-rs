/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    pin::Pin,
    task::{ready, Context, Poll},
};

use http_body::{Body, Frame, SizeHint};

use super::state::Finisher;

pin_project_lite::pin_project! {
    /// A response body that finishes the request once the body has been fully sent, sending it
    /// has failed, or the body is dropped.
    pub(crate) struct MetricsFinalizingBody<B> {
        #[pin]
        inner: B,
        finisher: Finisher,
    }
}

impl<B> MetricsFinalizingBody<B> {
    pub(crate) fn new(inner: B, finisher: Finisher) -> Self {
        Self { inner, finisher }
    }
}

impl<B: Body> Body for MetricsFinalizingBody<B> {
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.project();
        let frame = ready!(this.inner.poll_frame(cx));
        if matches!(frame, None | Some(Err(_))) {
            this.finisher.finish();
        }
        Poll::Ready(frame)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}
