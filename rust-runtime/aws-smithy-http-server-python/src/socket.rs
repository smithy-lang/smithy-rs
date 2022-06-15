/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Socket implementation that can be shared between multiple Python processes.

use pyo3::prelude::*;

use socket2::{Domain, Protocol, Socket, Type};
use std::net::SocketAddr;

/// Socket implementation that can be shared between multiple Python processes.
///
/// Python cannot handle true multi-threaded applications due to the [GIL],
/// often resulting in reduced performance and only one core used by the application.
/// To work around this, Python web applications usually create a socket with
/// SO_REUSEADDR and SO_REUSEPORT enabled that can be shared between multiple
/// Python processes, allowing you to maximize performance and use all available
/// computing capacity of the host.
///
/// [GIL]: https://wiki.python.org/moin/GlobalInterpreterLock
#[pyclass]
#[derive(Debug)]
pub struct SharedSocket {
    pub(crate) inner: Socket,
}

#[pymethods]
impl SharedSocket {
    /// Create a new UNIX `SharedSocket` from an address, port and backlog.
    /// If not specified, the backlog defaults to 1024 connections.
    #[new]
    #[cfg(not(target_os = "windows"))]
    pub fn new(address: String, port: i32, backlog: Option<i32>) -> PyResult<Self> {
        let address: SocketAddr = format!("{}:{}", address, port).parse()?;
        let domain = if address.is_ipv6() {
            Domain::IPV6
        } else {
            Domain::IPV4
        };
        tracing::info!("Shared socket listening on {address}, IP version: {domain:?}");
        let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
        // Set value for the `SO_REUSEPORT` and `SO_REUSEADDR` options on this socket.
        // This indicates that further calls to `bind` may allow reuse of local
        // addresses. For IPv4 sockets this means that a socket may bind even when
        // there's a socket already listening on this port.
        socket.set_reuse_port(true)?;
        socket.set_reuse_address(true)?;
        socket.bind(&address.into())?;
        socket.listen(backlog.unwrap_or(1024))?;
        Ok(SharedSocket { inner: socket })
    }

    /// Create a new Windows `SharedSocket` from an address, port and backlog.
    /// If not specified, the backlog defaults to 1024 connections.
    #[new]
    #[cfg(target_os = "windows")]
    pub fn new(address: String, port: i32, backlog: Option<i32>) -> PyResult<Self> {
        let address: SocketAddr = format!("{}:{}", address, port).parse()?;
        let domain = if address.is_ipv6() {
            Domain::IPV6
        } else {
            Domain::IPV4
        };
        tracing::info!("Shared socket listening on {address}, IP version: {domain:?}");
        let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
        // `SO_REUSEPORT` is not available on Windows.
        socket.set_reuse_address(true)?;
        socket.bind(&address.into())?;
        socket.listen(backlog.unwrap_or(1024))?;
        Ok(SharedSocket { inner: socket })
    }

    /// Clone the inner socket allowing it to be shared between multiple
    /// Python processes.
    #[pyo3(text_signature = "($self, socket, worker_number)")]
    pub fn try_clone(&self) -> PyResult<SharedSocket> {
        let copied = self.inner.try_clone()?;
        Ok(SharedSocket { inner: copied })
    }
}

impl SharedSocket {
    /// Get a cloned inner socket.
    pub fn get_socket(&self) -> Result<Socket, std::io::Error> {
        self.inner.try_clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_can_bind_on_random_port() {
        let _socket = SharedSocket::new("127.0.0.1".to_owned(), 0, None).unwrap();
        #[cfg(not(target_os = "windows"))]
        assert!(_socket.inner.is_listener().is_ok());
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn socket_can_be_cloned() {
        let socket = SharedSocket::new("127.0.0.1".to_owned(), 0, None).unwrap();
        let _cloned_socket = socket.try_clone().unwrap();
        #[cfg(not(target_os = "windows"))]
        assert!(_cloned_socket.inner.is_listener().is_ok());
    }
}
