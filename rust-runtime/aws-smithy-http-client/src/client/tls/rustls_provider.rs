/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use crate::client::tls::Provider;
use rustls::crypto::CryptoProvider;

/// Choice of underlying cryptography library (this only applies to rustls)
#[derive(Debug, Eq, PartialEq, Clone)]
#[non_exhaustive]
pub enum CryptoMode {
    /// Crypto based on [ring](https://github.com/briansmith/ring)
    #[cfg(feature = "rustls-ring")]
    Ring,
    /// Crypto based on [aws-lc](https://github.com/aws/aws-lc-rs)
    #[cfg(feature = "rustls-aws-lc")]
    AwsLc,
    /// FIPS compliant variant of [aws-lc](https://github.com/aws/aws-lc-rs)
    #[cfg(feature = "rustls-aws-lc-fips")]
    AwsLcFips,
}

impl CryptoMode {
    fn provider(self) -> CryptoProvider {
        match self {
            #[cfg(feature = "rustls-aws-lc")]
            CryptoMode::AwsLc => rustls::crypto::aws_lc_rs::default_provider(),

            #[cfg(feature = "rustls-ring")]
            CryptoMode::Ring => rustls::crypto::ring::default_provider(),

            #[cfg(feature = "rustls-aws-lc-fips")]
            CryptoMode::AwsLcFips => {
                let provider = rustls::crypto::default_fips_provider();
                assert!(
                    provider.fips(),
                    "FIPS was requested but the provider did not support FIPS"
                );
                provider
            }
        }
    }
}

impl Provider {
    /// Create a TLS provider based on [rustls](https://github.com/rustls/rustls)
    /// and the given [`CryptoMode`]
    pub fn rustls(mode: CryptoMode) -> Provider {
        Provider::Rustls(mode)
    }
}

pub(crate) mod build_connector {
    use crate::client::tls::rustls_provider::CryptoMode;
    use crate::tls::TlsContext;
    use client::connect::HttpConnector;
    use hyper_util::client::legacy as client;
    use rustls::crypto::CryptoProvider;
    use rustls_native_certs::CertificateResult;
    use rustls_pki_types::pem::PemObject;
    use rustls_pki_types::CertificateDer;
    use std::sync::Arc;
    use std::sync::LazyLock;

    /// Cached native certificates
    ///
    /// Creating a `with_native_roots()` hyper_rustls client re-loads system certs
    /// each invocation (which can take 300ms on OSx). Cache the loaded certs
    /// to avoid repeatedly incurring that cost.
    pub(crate) static NATIVE_ROOTS: LazyLock<Vec<CertificateDer<'static>>> = LazyLock::new(|| {
        let CertificateResult { certs, errors, .. } = rustls_native_certs::load_native_certs();
        if !errors.is_empty() {
            tracing::warn!("native root CA certificate loading errors: {errors:?}")
        }

        if certs.is_empty() {
            tracing::warn!("no native root CA certificates found!");
        }

        // NOTE: unlike hyper-rustls::with_native_roots we don't validate here, we'll do that later
        // for now we have a collection of certs that may or may not be valid.
        certs
    });

    pub(crate) fn restrict_ciphers(base: CryptoProvider) -> CryptoProvider {
        let suites = &[
            rustls::CipherSuite::TLS13_AES_256_GCM_SHA384,
            rustls::CipherSuite::TLS13_AES_128_GCM_SHA256,
            // TLS1.2 suites
            rustls::CipherSuite::TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
            rustls::CipherSuite::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
            rustls::CipherSuite::TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
            rustls::CipherSuite::TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
            rustls::CipherSuite::TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
        ];
        let supported_suites = suites
            .iter()
            .flat_map(|suite| {
                base.cipher_suites
                    .iter()
                    .find(|s| &s.suite() == suite)
                    .cloned()
            })
            .collect::<Vec<_>>();
        CryptoProvider {
            cipher_suites: supported_suites,
            ..base
        }
    }

    impl TlsContext {
        pub(crate) fn rustls_root_certs(&self) -> rustls::RootCertStore {
            let mut roots = rustls::RootCertStore::empty();
            if self.trust_store.enable_native_roots {
                let (valid, _invalid) = roots.add_parsable_certificates(NATIVE_ROOTS.clone());
                debug_assert!(valid > 0, "TrustStore configured to enable native roots but no valid root certificates parsed!");
            }

            for pem_cert in &self.trust_store.custom_certs {
                let ders = CertificateDer::pem_slice_iter(&pem_cert.0)
                    .collect::<Result<Vec<_>, _>>()
                    .expect("valid PEM certificate");
                for cert in ders {
                    roots.add(cert).expect("cert parsable")
                }
            }

            roots
        }
    }

    /// Create a rustls ClientConfig with smithy-rs defaults
    ///
    /// This centralizes the rustls ClientConfig creation logic to ensure
    /// consistency between the main HTTPS connector and tunnel handlers.
    pub(crate) fn create_rustls_client_config(
        crypto_mode: CryptoMode,
        tls_context: &TlsContext,
    ) -> rustls::ClientConfig {
        let root_certs = tls_context.rustls_root_certs();
        rustls::ClientConfig::builder_with_provider(Arc::new(restrict_ciphers(crypto_mode.provider())))
            .with_safe_default_protocol_versions()
            .expect("Error with the TLS configuration. Please file a bug report under https://github.com/smithy-lang/smithy-rs/issues.")
            .with_root_certificates(root_certs)
            .with_no_client_auth()
    }

    pub(crate) fn wrap_connector<R>(
        mut conn: HttpConnector<R>,
        crypto_mode: CryptoMode,
        tls_context: &TlsContext,
    ) -> hyper_rustls::HttpsConnector<HttpConnector<R>> {
        let client_config = create_rustls_client_config(crypto_mode, tls_context);
        conn.enforce_http(false);
        hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(client_config)
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .wrap_connector(conn)
    }
}

mod connect {
    use crate::client::connect::{Conn, Connecting};
    use aws_smithy_runtime_api::box_error::BoxError;
    use http_1x::Uri;
    use hyper::rt::{Read, ReadBufCursor, Write};
    use hyper_rustls::MaybeHttpsStream;
    use hyper_util::client::legacy::connect::{Connected, Connection, HttpConnector};
    use hyper_util::rt::TokioIo;
    use pin_project_lite::pin_project;
    use std::sync::Arc;
    use std::{
        io::{self, IoSlice},
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::io::{AsyncRead, AsyncWrite};
    use tokio::net::TcpStream;
    use tokio_rustls::client::TlsStream;

    #[derive(Debug, Clone)]
    pub(super) struct RustTlsConnector<R> {
        https: hyper_rustls::HttpsConnector<HttpConnector<R>>,
        tls_config: Arc<rustls::ClientConfig>,
    }

    impl<R> tower::Service<Uri> for RustTlsConnector<R> {
        type Response = Conn;
        type Error = BoxError;
        type Future = Connecting;

        fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, dst: Uri) -> Self::Future {
            todo!()
        }
    }

    pin_project! {
        pub(super) struct RustTlsConn<T> {
            #[pin] pub(super) inner: TokioIo<TlsStream<T>>
        }
    }

    impl Connection for RustTlsConn<TokioIo<TokioIo<TcpStream>>> {
        fn connected(&self) -> Connected {
            if self.inner.inner().get_ref().1.alpn_protocol() == Some(b"h2") {
                self.inner
                    .inner()
                    .get_ref()
                    .0
                    .inner()
                    .connected()
                    .negotiated_h2()
            } else {
                self.inner.inner().get_ref().0.inner().connected()
            }
        }
    }

    impl Connection for RustTlsConn<TokioIo<MaybeHttpsStream<TokioIo<TcpStream>>>> {
        fn connected(&self) -> Connected {
            if self.inner.inner().get_ref().1.alpn_protocol() == Some(b"h2") {
                self.inner
                    .inner()
                    .get_ref()
                    .0
                    .inner()
                    .connected()
                    .negotiated_h2()
            } else {
                self.inner.inner().get_ref().0.inner().connected()
            }
        }
    }
    impl<T: AsyncRead + AsyncWrite + Unpin> Read for RustTlsConn<T> {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context,
            buf: ReadBufCursor<'_>,
        ) -> Poll<tokio::io::Result<()>> {
            let this = self.project();
            Read::poll_read(this.inner, cx, buf)
        }
    }

    impl<T: AsyncRead + AsyncWrite + Unpin> Write for RustTlsConn<T> {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &[u8],
        ) -> Poll<Result<usize, tokio::io::Error>> {
            let this = self.project();
            Write::poll_write(this.inner, cx, buf)
        }

        fn poll_write_vectored(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bufs: &[IoSlice<'_>],
        ) -> Poll<Result<usize, io::Error>> {
            let this = self.project();
            Write::poll_write_vectored(this.inner, cx, bufs)
        }

        fn is_write_vectored(&self) -> bool {
            self.inner.is_write_vectored()
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            cx: &mut Context,
        ) -> Poll<Result<(), tokio::io::Error>> {
            let this = self.project();
            Write::poll_flush(this.inner, cx)
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            cx: &mut Context,
        ) -> Poll<Result<(), tokio::io::Error>> {
            let this = self.project();
            Write::poll_shutdown(this.inner, cx)
        }
    }
}
