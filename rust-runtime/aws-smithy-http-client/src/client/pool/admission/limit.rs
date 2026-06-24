/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Connection cap enforcement: acquires per-host then global semaphore
//! permits before allowing an inner service to proceed.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use aws_smithy_runtime_api::box_error::BoxError;
use tokio::sync::Semaphore;
use tower::Service;

use super::super::connection::{AcquireMode, ConnectCtx, ConnectionPermit, EstablishedConnection};
use super::super::stats::EstablishingGuard;
use super::super::PeerReclaimHandle;

/// Acquires per-host then global semaphore permits, enforcing the pool's
/// connection cap. Assembles [`EstablishedConnection`] on the unwind by
/// attaching the permit to the inner service's `(IO, EstablishingGuard)`
/// output.
pub(crate) struct ConnectionLimit<S> {
    inner: S,
    global: Option<Arc<Semaphore>>,
    per_host: Option<Arc<Semaphore>>,
    reclaim: Option<PeerReclaimHandle>,
}

impl<S> ConnectionLimit<S> {
    pub(crate) fn new(
        inner: S,
        global: Option<Arc<Semaphore>>,
        per_host: Option<Arc<Semaphore>>,
        reclaim: Option<PeerReclaimHandle>,
    ) -> Self {
        Self {
            inner,
            global,
            per_host,
            reclaim,
        }
    }
}

impl<S: Clone> Clone for ConnectionLimit<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            global: self.global.clone(),
            per_host: self.per_host.clone(),
            reclaim: self.reclaim.clone(),
        }
    }
}

impl<S, IO> Service<ConnectCtx> for ConnectionLimit<S>
where
    S: Service<ConnectCtx, Response = (IO, EstablishingGuard)> + Clone + Send + 'static,
    S::Error: Into<BoxError> + 'static,
    S::Future: Send + 'static,
    IO: Send + 'static,
{
    type Response = EstablishedConnection<IO>;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, ctx: ConnectCtx) -> Self::Future {
        let mut inner = self.inner.clone();
        let global = self.global.clone();
        let per_host = self.per_host.clone();
        let reclaim = self.reclaim.clone();
        // The future is boxed: acquiring per-host then global permits is
        // multi-phase async. This runs once per new connection, never on
        // connection reuse.
        Box::pin(async move {
            let mode = ctx.mode;
            // Per-host before global: never hold a global permit while
            // waiting on a per-host permit.
            let per_host_permit = match &per_host {
                Some(sem) => Some(
                    acquire_or_reclaim(sem, reclaim.as_ref(), mode, || {
                        let key = super::super::PoolKey::from_uri(&ctx.uri)
                            .expect("connect URI has scheme+authority");
                        super::super::BindingConstraint::PerHost(key)
                    })
                    .await?,
                ),
                None => None,
            };
            let global_permit = match &global {
                Some(sem) => Some(
                    acquire_or_reclaim(sem, reclaim.as_ref(), mode, || {
                        super::super::BindingConstraint::Global
                    })
                    .await?,
                ),
                None => None,
            };
            let permit = Arc::new(ConnectionPermit::new(global_permit, per_host_permit));

            let (io, establishing) = inner.call(ctx).await.map_err(Into::into)?;
            Ok(EstablishedConnection {
                io,
                permit,
                establishing,
            })
        })
    }
}

/// Acquire one owned permit. Fast path is `try_acquire_owned`. On
/// `NoPermits`: under [`AcquireMode::NonBlocking`] return [`CapBound`]
/// immediately (the caller will try a peer borrow); under
/// [`AcquireMode::Blocking`] free one peer's idle connection for
/// `constraint` (inline, best-effort) then blocking-acquire.
async fn acquire_or_reclaim(
    sem: &Arc<Semaphore>,
    reclaim: Option<&PeerReclaimHandle>,
    mode: AcquireMode,
    constraint: impl FnOnce() -> super::super::BindingConstraint,
) -> Result<tokio::sync::OwnedSemaphorePermit, BoxError> {
    match sem.clone().try_acquire_owned() {
        Ok(permit) => Ok(permit),
        Err(tokio::sync::TryAcquireError::NoPermits) => {
            if mode == AcquireMode::NonBlocking {
                return Err(super::super::connection::CapBound.into());
            }
            if let Some(reclaim) = reclaim {
                reclaim.try_free_under_load(&constraint());
            }
            sem.clone()
                .acquire_owned()
                .await
                .map_err(|_| "pool closed".into())
        }
        Err(tokio::sync::TryAcquireError::Closed) => Err("pool closed".into()),
    }
}
