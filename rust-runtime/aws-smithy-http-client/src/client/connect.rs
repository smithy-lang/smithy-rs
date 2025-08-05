/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use aws_smithy_runtime_api::box_error::BoxError;
use hyper::rt::{Read, ReadBufCursor, Write};
use hyper_util::client::legacy::connect::{Connected, Connection};
use pin_project_lite::pin_project;
use std::future::Future;
use std::io;
use std::io::IoSlice;
use std::pin::Pin;
use std::task::{Context, Poll};

pub(crate) trait AsyncConn:
    Read + Write + Connection + Send + Sync + Unpin + 'static
{
}

pub(crate) type BoxConn = Box<dyn AsyncConn>;

// Future for connecting
pub(crate) type Connecting = Pin<Box<dyn Future<Output = Result<Conn, BoxError>> + Send>>;

pin_project! {
    pub(crate) struct Conn {
        #[pin]
        pub(super)inner: BoxConn,
        pub(super) is_proxy: bool,
    }
}

impl Connection for Conn {
    fn connected(&self) -> Connected {
        let connected = self.inner.connected().proxy(self.is_proxy);
        connected
    }
}

impl Read for Conn {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.project();
        Read::poll_read(this.inner, cx, buf)
    }
}

impl Write for Conn {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
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

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let this = self.project();
        Write::poll_flush(this.inner, cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let this = self.project();
        Write::poll_shutdown(this.inner, cx)
    }
}
