/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::pin::Pin;
use std::sync::mpsc;
use std::task::{Context, Poll};

use futures::{ready, Stream};
use hyper::server::accept::Accept;
use pin_project_lite::pin_project;
use tls_listener::{AsyncAccept, AsyncTls, Error as TlsListenerError, TlsListener};

pin_project! {
    /// A wrapper around [TlsListener] that allows changing TLS config via a channel
    /// and ignores incorrect connections (they cause Hyper server to shutdown otherwise).
    pub struct Listener<A: AsyncAccept, T: AsyncTls<A::Connection>> {
        #[pin]
        inner: TlsListener<A, T>,
        new_acceptor_rx: mpsc::Receiver<T>,
    }
}

impl<A: AsyncAccept, T: AsyncTls<A::Connection>> Listener<A, T> {
    pub fn new(tls: T, listener: A, new_acceptor_rx: mpsc::Receiver<T>) -> Self {
        Self {
            inner: TlsListener::new(tls, listener),
            new_acceptor_rx,
        }
    }
}

impl<A, T> Accept for Listener<A, T>
where
    A: AsyncAccept,
    A::Error: std::error::Error,
    T: AsyncTls<A::Connection>,
{
    type Conn = T::Stream;
    type Error = A::Error;

    fn poll_accept(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Conn, Self::Error>>> {
        // Replace current acceptor (it also contains TLS config) if there is a new one
        if let Ok(acceptor) = self.new_acceptor_rx.try_recv() {
            self.as_mut().project().inner.replace_acceptor_pin(acceptor);
        }

        loop {
            match ready!(self.as_mut().project().inner.poll_next(cx)) {
                Some(Ok(conn)) => return Poll::Ready(Some(Ok(conn))),
                Some(Err(TlsListenerError::ListenerError(err))) => {
                    return Poll::Ready(Some(Err(err)))
                }
                Some(Err(TlsListenerError::TlsAcceptError(err))) => {
                    // Don't propogate TLS handshake errors to Hyper because it causes server to shutdown
                    tracing::debug!(error = ?err, "tls handshake error");
                }
                None => return Poll::Ready(None),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::net::SocketAddr;
    use std::pin::Pin;
    use std::sync::{mpsc, Arc};
    use std::task::{Context, Poll};
    use std::thread;

    use futures::ready;
    use hyper::server::conn::{AddrIncoming, AddrStream};
    use hyper::service::{make_service_fn, service_fn};
    use hyper::{Body, Client, Error, Response, Server, Uri};
    use hyper_rustls::HttpsConnectorBuilder;
    use pin_project_lite::pin_project;
    use tls_listener::AsyncAccept;
    use tokio_rustls::{
        rustls::{Certificate, ClientConfig, PrivateKey, RootCertStore, ServerConfig},
        TlsAcceptor,
    };

    use super::Listener;

    enum DummyListenerMode {
        // Pass connection from inner `AddrIncoming` without any modification
        Identity,
        // Fail after accepting a connection from inner `AddrIncoming`
        Fail,
    }

    pin_project! {
        // A listener for testing that uses inner `AddrIncoming` to accept connections
        // and depending on the mode it either returns that connection or fails.
        struct DummyListener {
            #[pin]
            inner: AddrIncoming,
            mode: DummyListenerMode,
        }
    }

    impl AsyncAccept for DummyListener {
        type Connection = AddrStream;
        type Error = io::Error;

        fn poll_accept(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Self::Connection, Self::Error>>> {
            let this = self.project();
            let conn = match ready!(this.inner.poll_accept(cx)) {
                Some(Ok(conn)) => conn,
                Some(Err(err)) => return Poll::Ready(Some(Err(err))),
                None => return Poll::Ready(None),
            };

            match &this.mode {
                DummyListenerMode::Identity => Poll::Ready(Some(Ok(conn))),
                DummyListenerMode::Fail => {
                    Poll::Ready(Some(Err(io::ErrorKind::ConnectionAborted.into())))
                }
            }
        }
    }

    #[tokio::test]
    async fn server_doesnt_shutdown_after_bad_handshake() {
        let (_new_acceptor_tx, new_acceptor_rx) = mpsc::channel();
        let cert = valid_cert();
        let acceptor = acceptor_from_cert(&cert);
        let (addr, _) = server(acceptor, new_acceptor_rx, DummyListenerMode::Identity);

        {
            // Here client only trusts the `different_cert` and fails for any other certificate even though they are valid
            let different_cert = valid_cert();
            let config = client_config_with_cert(&different_cert);
            let response = make_req(config, &addr).await;
            assert!(response
                .unwrap_err()
                .to_string()
                .contains("invalid peer certificate"));
        }

        {
            // Now use the same cert and it should succeed
            let config = client_config_with_cert(&cert);
            let response = make_req(config, &addr).await.unwrap();
            assert_eq!(
                "hello world",
                hyper::body::to_bytes(response.into_body()).await.unwrap()
            );
        }
    }

    #[tokio::test]
    #[should_panic(expected = "server error: error accepting connection: connection aborted")]
    async fn server_shutdown_after_listener_error() {
        let (_new_acceptor_tx, new_acceptor_rx) = mpsc::channel();
        let cert = valid_cert();
        let acceptor = acceptor_from_cert(&cert);
        let (addr, server_thread_handle) =
            server(acceptor, new_acceptor_rx, DummyListenerMode::Fail);

        // Here server should get an error from listener
        let config = client_config_with_cert(&cert);
        let _ = make_req(config, &addr).await;

        // Since we are just panicking in our test server, we just need to propogate that panic
        // and `should_panic` will make sure it is the panic message we are expecting
        std::panic::resume_unwind(server_thread_handle.join().unwrap_err());
    }

    #[tokio::test]
    async fn server_changes_tls_config() {
        let (new_acceptor_tx, new_acceptor_rx) = mpsc::channel();

        let invalid_cert = cert_with_invalid_date();
        let acceptor = acceptor_from_cert(&invalid_cert);
        let (addr, _) = server(acceptor, new_acceptor_rx, DummyListenerMode::Identity);

        {
            // We have a certificate with invalid date, so request should fail
            let config = client_config_with_cert(&invalid_cert);
            let response = make_req(config, &addr).await;
            assert!(response
                .unwrap_err()
                .to_string()
                .contains("invalid peer certificate: Expired"));
        }

        // Make a new acceptor with a valid cert and replace
        let cert = valid_cert();
        let acceptor = acceptor_from_cert(&cert);
        tokio::spawn(async move {
            new_acceptor_tx.send(acceptor).unwrap();
        });

        {
            // Now it should succeed
            let config = client_config_with_cert(&cert);
            let response = make_req(config, &addr).await.unwrap();
            assert_eq!(
                "hello world",
                hyper::body::to_bytes(response.into_body()).await.unwrap()
            );
        }
    }

    fn client_config_with_cert(cert: &rcgen::Certificate) -> ClientConfig {
        let mut roots = RootCertStore::empty();
        roots.add_parsable_certificates(&[cert.serialize_der().unwrap()]);
        ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(roots)
            .with_no_client_auth()
    }

    fn cert_with_invalid_date() -> rcgen::Certificate {
        let mut params = rcgen::CertificateParams::new(vec!["localhost".to_string()]);
        params.not_after = rcgen::date_time_ymd(1970, 1, 1);
        rcgen::Certificate::from_params(params).unwrap()
    }

    fn valid_cert() -> rcgen::Certificate {
        let params = rcgen::CertificateParams::new(vec!["localhost".to_string()]);
        rcgen::Certificate::from_params(params).unwrap()
    }

    fn acceptor_from_cert(cert: &rcgen::Certificate) -> TlsAcceptor {
        TlsAcceptor::from(Arc::new(
            ServerConfig::builder()
                .with_safe_defaults()
                .with_no_client_auth()
                .with_single_cert(
                    vec![Certificate(cert.serialize_der().unwrap())],
                    PrivateKey(cert.serialize_private_key_der()),
                )
                .unwrap(),
        ))
    }

    fn server(
        acceptor: TlsAcceptor,
        new_acceptor_rx: mpsc::Receiver<TlsAcceptor>,
        dummy_listener_mode: DummyListenerMode,
    ) -> (SocketAddr, thread::JoinHandle<()>) {
        let addr = ([127, 0, 0, 1], 0).into();
        let (addr_tx, addr_rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            tokio_test::block_on(async move {
                let incoming = AddrIncoming::bind(&addr).unwrap();
                addr_tx.send(incoming.local_addr()).unwrap();

                let incoming = DummyListener {
                    inner: incoming,
                    mode: dummy_listener_mode,
                };

                let listener = Listener::new(acceptor, incoming, new_acceptor_rx);

                let make_svc = make_service_fn(|_| async {
                    Ok::<_, Error>(service_fn(|_req| async {
                        Ok::<_, Error>(Response::new(Body::from("hello world")))
                    }))
                });
                let server = Server::builder(listener).serve(make_svc);
                if let Err(err) = server.await {
                    panic!("server error: {err}");
                }
            });
        });

        (addr_rx.recv().unwrap(), handle)
    }

    async fn make_req(
        config: ClientConfig,
        addr: &SocketAddr,
    ) -> Result<Response<Body>, hyper::Error> {
        let connector = HttpsConnectorBuilder::new()
            .with_tls_config(config)
            .https_only()
            .enable_http2()
            .build();

        let client = Client::builder().build::<_, Body>(connector);
        client
            .get(
                Uri::builder()
                    .scheme("https")
                    .authority(format!("localhost:{}", addr.port()))
                    .path_and_query("/")
                    .build()
                    .unwrap(),
            )
            .await
    }
}
