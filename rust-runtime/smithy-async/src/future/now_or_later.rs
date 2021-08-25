/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use pin_project_lite::pin_project;

pin_project! {
    /// Future with an explicit "Now" variant
    ///
    /// When a future is immediately, ready, this enables avoiding an unecessary allocation.
    /// This is intended to be used with `Pin<Box<dyn Future>>` or similar as the future variant.
    pub struct NowOrLater<T, F> {
        #[pin]
        inner: Inner<T, F>
    }
}

pin_project! {
    #[project = NowOrLaterProj]
    enum Inner<T, F> {
        #[non_exhaustive]
        Now { value: Option<T> },
        #[non_exhaustive]
        Later { #[pin] future: F },
    }
}

impl<T, F> NowOrLater<T, F> {
    pub fn new(future: F) -> Self {
        Self {
            inner: Inner::Later { future },
        }
    }

    pub fn ready(value: T) -> Self {
        let value = Some(value);
        Self {
            inner: Inner::Now { value },
        }
    }
}

impl<T, F> Future for NowOrLater<T, F>
where
    F: Future<Output = T>,
{
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().inner.project() {
            NowOrLaterProj::Now { value } => {
                Poll::Ready(value.take().expect("cannot be called twice"))
            }
            NowOrLaterProj::Later { future } => future.poll(cx),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::future::now_or_later::NowOrLater;
    use std::future::Ready;

    #[test]
    fn ready_future_immediately_returns() {
        let f = NowOrLater::<_, Ready<i32>>::ready(5);
        use futures_util::FutureExt;
        assert_eq!(f.now_or_never().expect("future was ready"), 5);
    }

    #[tokio::test]
    async fn box_dyn_future() {
        let f = async { 5 };
        let f = Box::pin(f);
        let wrapped = NowOrLater::new(f);
        assert_eq!(wrapped.await, 5);
    }

    #[tokio::test]
    async fn async_fn_future() {
        let wrapped = NowOrLater::new(async { 5 });
        assert_eq!(wrapped.await, 5);
    }
}
