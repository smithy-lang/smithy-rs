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
///
/// :param address str:
/// :param port int:
/// :param backlog typing.Optional\[int\]:
/// :rtype None:
#[pyclass]
#[derive(Debug)]
pub struct PySocket {
    pub(crate) inner: Socket,
}

#[pymethods]
impl PySocket {
    /// Create a new UNIX `SharedSocket` from an address, port and backlog.
    /// If not specified, the backlog defaults to 1024 connections.
    #[pyo3(text_signature = "($self, address, port, backlog=None)")]
    #[new]
    pub fn new(address: String, port: i32, backlog: Option<i32>) -> PyResult<Self> {
        let address: SocketAddr = format!("{}:{}", address, port).parse()?;
        let (domain, ip_version) = PySocket::socket_domain(address);
        tracing::trace!(address = %address, ip_version, "shared socket listening");
        let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
        // Set value for the `SO_REUSEPORT` and `SO_REUSEADDR` options on this socket.
        // This indicates that further calls to `bind` may allow reuse of local
        // addresses. For IPv4 sockets this means that a socket may bind even when
        // there's a socket already listening on this port.
        socket.set_reuse_port(true)?;
        socket.set_reuse_address(true)?;
        socket.bind(&address.into())?;
        socket.listen(backlog.unwrap_or(1024))?;
        Ok(PySocket { inner: socket })
    }

    /// Clone the inner socket allowing it to be shared between multiple
    /// Python processes.
    ///
    /// :rtype PySocket:
    pub fn try_clone(&self) -> PyResult<PySocket> {
        let copied = self.inner.try_clone()?;
        Ok(PySocket { inner: copied })
    }
}

impl PySocket {
    /// Get a cloned inner socket.
    pub fn get_socket(&self) -> Result<Socket, std::io::Error> {
        self.inner.try_clone()
    }

    /// Find the socket domain
    fn socket_domain(address: SocketAddr) -> (Domain, &'static str) {
        if address.is_ipv6() {
            (Domain::IPV6, "6")
        } else {
            (Domain::IPV4, "4")
        }
    }
}

#[cfg(test)]
// `is_listener` on `Socket` is only available on certain platforms.
// In particular, this fails to compile on MacOS.
#[cfg(any(
    target_os = "android",
    target_os = "freebsd",
    target_os = "fuchsia",
    target_os = "linux",
))]
mod tests {
    use super::*;

    #[test]
    fn socket_can_bind_on_random_port() {
        let socket = PySocket::new("127.0.0.1".to_owned(), 0, None).unwrap();
        assert!(socket.inner.is_listener().is_ok());
    }

    #[test]
    fn socket_can_be_cloned() {
        let socket = PySocket::new("127.0.0.1".to_owned(), 0, None).unwrap();
        let cloned_socket = socket.try_clone().unwrap();
        assert!(cloned_socket.inner.is_listener().is_ok());
    }
}
