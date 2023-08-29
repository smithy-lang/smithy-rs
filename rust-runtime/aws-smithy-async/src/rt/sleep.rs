/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Provides an [`AsyncSleep`] trait that returns a future that sleeps for a given duration,
//! and implementations of `AsyncSleep` for different async runtimes.

use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

/// Async trait with a `sleep` function.
pub trait AsyncSleep: Debug + Send + Sync {
    /// Returns a future that sleeps for the given `duration` of time.
    fn sleep(&self, duration: Duration) -> Sleep;
}

impl<T> AsyncSleep for Box<T>
where
    T: AsyncSleep,
    T: ?Sized,
{
    fn sleep(&self, duration: Duration) -> Sleep {
        T::sleep(self, duration)
    }
}

impl<T> AsyncSleep for Arc<T>
where
    T: AsyncSleep,
    T: ?Sized,
{
    fn sleep(&self, duration: Duration) -> Sleep {
        T::sleep(self, duration)
    }
}

/// Wrapper type for sharable `AsyncSleep`
#[derive(Clone, Debug)]
pub struct SharedAsyncSleep(Arc<dyn AsyncSleep>);

impl SharedAsyncSleep {
    /// Create a new `SharedAsyncSleep` from `AsyncSleep`
    pub fn new(sleep: impl AsyncSleep + 'static) -> Self {
        Self(Arc::new(sleep))
    }
}

impl AsRef<dyn AsyncSleep> for SharedAsyncSleep {
    fn as_ref(&self) -> &(dyn AsyncSleep + 'static) {
        self.0.as_ref()
    }
}

impl From<Arc<dyn AsyncSleep>> for SharedAsyncSleep {
    fn from(sleep: Arc<dyn AsyncSleep>) -> Self {
        SharedAsyncSleep(sleep)
    }
}

impl AsyncSleep for SharedAsyncSleep {
    fn sleep(&self, duration: Duration) -> Sleep {
        self.0.sleep(duration)
    }
}

#[cfg(all(
    feature = "rt-tokio",
    not(all(target_family = "wasm", target_os = "wasi"))
))]
/// Returns a default sleep implementation based on the features enabled
pub fn default_async_sleep() -> Option<SharedAsyncSleep> {
    Some(SharedAsyncSleep::from(sleep_tokio()))
}

#[cfg(all(target_family = "wasm", target_os = "wasi"))]
/// Returns a default sleep implementation based on the features enabled
pub fn default_async_sleep() -> Option<SharedAsyncSleep> {
    Some(SharedAsyncSleep::from(sleep_wasi()))
}

#[cfg(not(any(all(target_family = "wasm", target_os = "wasi"), feature = "rt-tokio")))]
/// Returns a default sleep implementation based on the features enabled
pub fn default_async_sleep() -> Option<SharedAsyncSleep> {
    None
}

/// Future returned by [`AsyncSleep`].
#[non_exhaustive]
#[must_use]
pub struct Sleep(Pin<Box<dyn Future<Output = ()> + Send + 'static>>);

impl Debug for Sleep {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sleep")
    }
}

impl Sleep {
    /// Create a new [`Sleep`] future
    ///
    /// The provided future will be Boxed.
    pub fn new(future: impl Future<Output = ()> + Send + 'static) -> Sleep {
        Sleep(Box::pin(future))
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.as_mut().poll(cx)
    }
}

/// Implementation of [`AsyncSleep`] for Tokio.
#[non_exhaustive]
#[cfg(all(
    feature = "rt-tokio",
    not(all(target_family = "wasm", target_os = "wasi"))
))]
#[derive(Debug, Default)]
pub struct TokioSleep;

#[cfg(all(
    feature = "rt-tokio",
    not(all(target_family = "wasm", target_os = "wasi"))
))]
impl TokioSleep {
    /// Create a new [`AsyncSleep`] implementation using the Tokio hashed wheel sleep implementation
    pub fn new() -> TokioSleep {
        Default::default()
    }
}

#[cfg(all(
    feature = "rt-tokio",
    not(all(target_family = "wasm", target_os = "wasi"))
))]
impl AsyncSleep for TokioSleep {
    fn sleep(&self, duration: Duration) -> Sleep {
        Sleep::new(tokio::time::sleep(duration))
    }
}

#[cfg(all(
    feature = "rt-tokio",
    not(all(target_family = "wasm", target_os = "wasi"))
))]
fn sleep_tokio() -> Arc<dyn AsyncSleep> {
    Arc::new(TokioSleep::new())
}

/// Implementation of [`AsyncSleep`] for WASI.
#[non_exhaustive]
#[cfg(all(target_family = "wasm", target_os = "wasi"))]
#[derive(Debug, Default)]
pub struct WasiSleep;

#[cfg(all(target_family = "wasm", target_os = "wasi"))]
impl WasiSleep {
    /// Create a new [`AsyncSleep`] implementation using the WASI sleep implementation
    pub fn new() -> WasiSleep {
        Default::default()
    }
}

#[cfg(all(target_family = "wasm", target_os = "wasi"))]
impl AsyncSleep for WasiSleep {
    fn sleep(&self, duration: std::time::Duration) -> Sleep {
        Sleep::new(Box::pin(async move {
            std::thread::sleep(duration);
        }))
    }
}

#[cfg(all(target_family = "wasm", target_os = "wasi"))]
fn sleep_wasi() -> Arc<dyn AsyncSleep> {
    Arc::new(WasiSleep::new())
}
