/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Maintain a cache of discovered endpoints

use aws_smithy_async::rt::sleep::AsyncSleep;
use aws_smithy_client::erase::boxclone::BoxFuture;
use aws_smithy_http::endpoint::{ResolveEndpoint, ResolveEndpointError};
use aws_smithy_types::endpoint::Endpoint;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::sync::oneshot::error::TryRecvError;
use tokio::sync::oneshot::{Receiver, Sender};

/// Endpoint reloader
#[must_use]
pub struct ReloadEndpoint {
    loader: Box<dyn Fn() -> BoxFuture<(Endpoint, SystemTime), ResolveEndpointError> + Send + Sync>,
    endpoint: Arc<Mutex<Option<ExpiringEndpoint>>>,
    error: Arc<Mutex<Option<ResolveEndpointError>>>,
    rx: Receiver<()>,
    sleep: Arc<dyn AsyncSleep>,
}

impl Debug for ReloadEndpoint {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReloadEndpoint").finish()
    }
}

impl ReloadEndpoint {
    /// Reload the endpoint once
    pub async fn reload_once(&self) {
        match (self.loader)().await {
            Ok((endpoint, expiry)) => {
                *self.endpoint.lock().unwrap() = Some(ExpiringEndpoint { endpoint, expiry })
            }
            Err(err) => *self.error.lock().unwrap() = Some(err),
        }
    }

    /// An infinite loop task that will reload the endpoint
    ///
    /// This task will terminate when the corresponding [`EndpointCache`] is dropped.
    pub async fn reload_task(mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(_) | Err(TryRecvError::Closed) => break,
                _ => {}
            }
            let should_reload = self
                .endpoint
                .lock()
                .unwrap()
                .as_ref()
                .map(|e| e.is_expired())
                .unwrap_or(true);
            if should_reload {
                self.reload_once().await;
            }
            self.sleep.sleep(Duration::from_secs(60)).await
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct EndpointCache {
    error: Arc<Mutex<Option<ResolveEndpointError>>>,
    endpoint: Arc<Mutex<Option<ExpiringEndpoint>>>,
    // When the sender is dropped, this allows the reload loop to stop
    _drop_guard: Arc<Sender<()>>,
}

impl<T> ResolveEndpoint<T> for EndpointCache {
    fn resolve_endpoint(&self, _params: &T) -> aws_smithy_http::endpoint::Result {
        self.resolve_endpoint()
    }
}

#[derive(Debug)]
struct ExpiringEndpoint {
    endpoint: Endpoint,
    expiry: SystemTime,
}

impl ExpiringEndpoint {
    fn is_expired(&self) -> bool {
        match SystemTime::now().duration_since(self.expiry) {
            Err(e) => true,
            Ok(t) => t < Duration::from_secs(120),
        }
    }
}

pub(crate) async fn create_cache<F>(
    loader_fn: impl Fn() -> F + Send + Sync + 'static,
    sleep: Arc<dyn AsyncSleep>,
) -> Result<(EndpointCache, ReloadEndpoint), ResolveEndpointError>
where
    F: Future<Output = Result<(Endpoint, SystemTime), ResolveEndpointError>> + Send + 'static,
{
    let error_holder = Arc::new(Mutex::new(None));
    let endpoint_holder = Arc::new(Mutex::new(None));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let cache = EndpointCache {
        error: error_holder.clone(),
        endpoint: endpoint_holder.clone(),
        _drop_guard: Arc::new(tx),
    };
    let reloader = ReloadEndpoint {
        loader: Box::new(move || Box::pin((loader_fn)()) as _),
        endpoint: endpoint_holder,
        error: error_holder,
        rx,
        sleep,
    };
    reloader.reload_once().await;
    if let Err(e) = cache.resolve_endpoint() {
        return Err(e);
    }
    Ok((cache, reloader))
}

impl EndpointCache {
    fn resolve_endpoint(&self) -> aws_smithy_http::endpoint::Result {
        self.endpoint
            .lock()
            .unwrap()
            .as_ref()
            .map(|e| e.endpoint.clone())
            .ok_or_else(|| {
                self.error
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap_or_else(|| ResolveEndpointError::message("no endpoint loaded"))
            })
    }
}

#[cfg(test)]
mod test {
    use crate::endpoint_discovery::{create_cache, EndpointCache};
    use aws_smithy_async::rt::sleep::TokioSleep;
    use aws_smithy_http::endpoint::ResolveEndpointError;
    use std::sync::Arc;

    fn check_send<T: Send>() {}

    fn check_send_v<T: Send>(t: T) -> T {
        t
    }

    #[tokio::test]
    async fn check_traits() {
        // check_send::<EndpointCache>();

        let (cache, reloader) = create_cache(
            || async { Err(ResolveEndpointError::message("stub")) },
            Arc::new(TokioSleep::new()),
        )
        .await
        .unwrap();
        check_send_v(reloader.reload_task());
    }
}
