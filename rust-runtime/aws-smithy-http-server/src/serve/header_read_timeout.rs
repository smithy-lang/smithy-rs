/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::{
    error::Error as StdError,
    future::{poll_fn, Future},
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::Poll,
    time::Duration,
};

/// Observe the request at Hyper's service boundary, before the Tower adapter.
/// This also excludes Tower's readiness checks from the header deadline.
pub(super) struct HeaderReadTimeoutService<S> {
    inner: S,
    headers_received: Arc<AtomicBool>,
}

pub(super) struct HeaderReadDeadline {
    timeout: Duration,
    headers_received: Arc<AtomicBool>,
}

impl<S> HeaderReadTimeoutService<S> {
    pub(super) fn new(inner: S, timeout: Duration) -> (Self, HeaderReadDeadline) {
        debug_assert!(
            !timeout.is_zero(),
            "header timeout must be nonzero; zero disables the deadline"
        );
        let headers_received = Arc::new(AtomicBool::new(false));
        (
            Self {
                inner,
                headers_received: Arc::clone(&headers_received),
            },
            HeaderReadDeadline {
                timeout,
                headers_received,
            },
        )
    }
}

impl<S, R> hyper::service::Service<R> for HeaderReadTimeoutService<S>
where
    S: hyper::service::Service<R>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn call(&self, request: R) -> Self::Future {
        self.headers_received.store(true, Ordering::Release);
        self.inner.call(request)
    }
}

impl HeaderReadDeadline {
    /// Start timing on the first poll, after connection service creation and
    /// immediately before driving Hyper. No protocol bytes are inspected.
    pub(super) async fn run<C, E>(self, connection: C) -> io::Result<()>
    where
        C: Future<Output = Result<(), E>>,
        E: Into<Box<dyn StdError + Send + Sync>>,
    {
        let Self {
            timeout,
            headers_received,
        } = self;
        let mut connection = std::pin::pin!(connection);
        // Keep the optional timer inline; Pin::set drops it in place.
        let mut deadline = std::pin::pin!(Some(tokio::time::sleep(timeout)));

        poll_fn(|cx| {
            if let Some(timer) = deadline.as_mut().as_pin_mut() {
                if headers_received.load(Ordering::Acquire) {
                    deadline.as_mut().set(None);
                } else if timer.poll(cx).is_ready() {
                    tracing::trace!(timeout = ?timeout, "request headers timed out");
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "request headers timed out",
                    )));
                }
            }

            let result = connection.as_mut().poll(cx).map_err(io::Error::other);
            // Hyper may call the service and then return Pending while middleware
            // or the body is still waiting. Drop the timer in that same poll.
            if deadline.is_some() && headers_received.load(Ordering::Acquire) {
                deadline.as_mut().set(None);
            }
            result
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::{to_boxed, BoxBody};
    use http_body_util::BodyExt;
    use hyper::body::Incoming;
    use hyper_util::{
        rt::{TokioExecutor, TokioIo},
        server::conn::auto::Builder,
        service::TowerToHyperService,
    };
    use std::{convert::Infallible, pin::Pin, task::Context};
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
    use tower::{service_fn, Service};

    const REQUEST: &[u8] = b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    const TIMEOUT: Duration = Duration::from_secs(5);

    async fn start_server<S>(
        service: S,
        timeout: Option<Duration>,
    ) -> (DuplexStream, tokio::task::JoinHandle<io::Result<()>>)
    where
        S: Service<http::Request<Incoming>, Response = http::Response<BoxBody>, Error = Infallible>
            + Clone
            + Send
            + 'static,
        S::Future: Send,
    {
        let (server, client) = tokio::io::duplex(4096);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let service = TowerToHyperService::new(service);
            let mut builder = Builder::new(TokioExecutor::new());
            // Exercise our deadline independently of Hyper's HTTP/1 timer.
            builder.http1().header_read_timeout(None);
            if let Some(timeout) = timeout {
                let (service, deadline) = HeaderReadTimeoutService::new(service, timeout);
                let connection = builder.serve_connection_with_upgrades(TokioIo::new(server), service);
                started_tx.send(()).unwrap();
                deadline.run(connection).await
            } else {
                let connection = builder.serve_connection_with_upgrades(TokioIo::new(server), service);
                started_tx.send(()).unwrap();
                connection.await.map_err(io::Error::other)
            }
        });
        started_rx.await.unwrap();
        (client, task)
    }

    async fn ok(_request: http::Request<Incoming>) -> Result<http::Response<BoxBody>, Infallible> {
        Ok(http::Response::new(to_boxed("OK")))
    }

    async fn assert_ok(mut client: DuplexStream, task: tokio::task::JoinHandle<io::Result<()>>) {
        let mut response = Vec::new();
        client.read_to_end(&mut response).await.unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200 OK"), "{response:?}");
        task.await.unwrap().unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn incomplete_first_head_expires_for_either_protocol() {
        for prefix in [
            b"".as_slice(),
            b"GET / HTTP/1.1\r\nHost: localhost\r\n",
            b"PRI * HTTP/2.0\r\n",
            b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n",
        ] {
            let (mut client, task) = start_server(service_fn(ok), Some(TIMEOUT)).await;
            client.write_all(prefix).await.unwrap();
            tokio::task::yield_now().await;
            tokio::time::advance(TIMEOUT).await;
            let error = task.await.unwrap().unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::TimedOut, "prefix: {prefix:?}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn partial_bytes_do_not_restart_the_header_deadline() {
        let (mut client, task) = start_server(service_fn(ok), Some(TIMEOUT)).await;
        tokio::time::advance(Duration::from_secs(4)).await;
        client.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(task.await.unwrap().unwrap_err().kind(), io::ErrorKind::TimedOut);
    }

    #[tokio::test(start_paused = true)]
    async fn header_deadline_can_be_disabled() {
        let (mut client, task) = start_server(service_fn(ok), None).await;
        tokio::time::advance(Duration::from_secs(60)).await;
        client.write_all(REQUEST).await.unwrap();
        assert_ok(client, task).await;
    }

    #[tokio::test(start_paused = true)]
    async fn slow_request_middleware_is_excluded() {
        let service = service_fn(|_request: http::Request<Incoming>| async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok::<_, Infallible>(http::Response::new(to_boxed("OK")))
        });
        let (mut client, task) = start_server(service, Some(TIMEOUT)).await;
        client.write_all(REQUEST).await.unwrap();
        assert_ok(client, task).await;
    }

    #[tokio::test(start_paused = true)]
    async fn slow_request_body_is_excluded() {
        let service = service_fn(|request: http::Request<Incoming>| async {
            let body = request.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(body, "OK");
            Ok::<_, Infallible>(http::Response::new(to_boxed(body)))
        });
        let (mut client, task) = start_server(service, Some(TIMEOUT)).await;
        client
            .write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(10)).await;
        client.write_all(b"OK").await.unwrap();
        assert_ok(client, task).await;
    }

    struct SlowReady {
        ready: Option<Pin<Box<tokio::time::Sleep>>>,
    }

    impl Clone for SlowReady {
        fn clone(&self) -> Self {
            Self { ready: None }
        }
    }

    impl Service<http::Request<Incoming>> for SlowReady {
        type Response = http::Response<BoxBody>;
        type Error = Infallible;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            let delay = self
                .ready
                .get_or_insert_with(|| Box::pin(tokio::time::sleep(Duration::from_secs(10))));
            delay.as_mut().poll(cx).map(|()| Ok(()))
        }

        fn call(&mut self, _request: http::Request<Incoming>) -> Self::Future {
            std::future::ready(Ok(http::Response::new(to_boxed("OK"))))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn tower_readiness_is_excluded() {
        let (mut client, task) = start_server(SlowReady { ready: None }, Some(TIMEOUT)).await;
        client.write_all(REQUEST).await.unwrap();
        assert_ok(client, task).await;
    }
}
