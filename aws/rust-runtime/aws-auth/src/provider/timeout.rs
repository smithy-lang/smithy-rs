/*
 * Original Copyright (c) 2021 Tokio Contributors. Licensed under the Apache-2.0 license.
 * Modifications Copyright 2021 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use pin_project::pin_project;
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct TimedOutError;

impl Error for TimedOutError {}

impl fmt::Display for TimedOutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "timed out")
    }
}

#[pin_project]
#[must_use = "futures do nothing unless you `.await` or poll them"]
#[derive(Debug)]
pub struct Timeout<T, S> {
    #[pin]
    value: T,
    #[pin]
    sleep: S,
}

impl<T, S> Timeout<T, S> {
    pub(crate) fn new(value: T, sleep: S) -> Timeout<T, S> {
        Timeout { value, sleep }
    }
}

impl<T, S> Future for Timeout<T, S>
where
    T: Future,
    S: Future,
{
    type Output = Result<T::Output, TimedOutError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let me = self.project();

        // First, try polling the future
        if let Poll::Ready(v) = me.value.poll(cx) {
            return Poll::Ready(Ok(v));
        }

        // Now check the timer
        match me.sleep.poll(cx) {
            Poll::Ready(_) => Poll::Ready(Err(TimedOutError)),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Timeout;
    use crate::provider::timeout::TimedOutError;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    struct Never;
    impl Future for Never {
        type Output = ();

        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            Poll::Pending
        }
    }

    #[tokio::test]
    async fn success() {
        assert_eq!(
            Ok(Ok(5)),
            Timeout::new(async { Ok::<isize, isize>(5) }, Never).await
        );
    }

    #[tokio::test]
    async fn failure() {
        assert_eq!(
            Ok(Err(0)),
            Timeout::new(async { Err::<isize, isize>(0) }, Never).await
        );
    }

    #[tokio::test]
    async fn timeout() {
        assert_eq!(Err(TimedOutError), Timeout::new(Never, async {}).await);
    }

    // If the value is available at the same time as the timeout, then return the value
    #[tokio::test]
    async fn prefer_value_to_timeout() {
        assert_eq!(Ok(5), Timeout::new(async { 5 }, async {}).await);
    }
}
