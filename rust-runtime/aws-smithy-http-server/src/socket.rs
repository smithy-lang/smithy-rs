/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Socket implementation that can be shared between multiple processes.
//!
//! Applications running in single threaded interpreters like Python or Javascript
//! require one Tokio runtime and one Hyper server per CPU core to maximize the
//! throughput. SO_REUSEADDR and SO_REUSEPORT socket options allows multiple processes
//! to bind to a sigle socket where requests are balanced automatically by the kernel.
//! The implementation is based on the [`socket2`] crate.
//!
//! Language-dependent implementations will specialize over the [`socket2::Socket`] that is returned
//! by `new_socket`.
use socket2::{Domain, Protocol, Type};
use std::{error::Error, net::SocketAddr};

/// Create a new socket for an `address:port`. The connection backlog can be passed as an
/// optional argument.
///
/// # Errors
///
/// This function will return an error if the socket cannot be bound. This can usually happen if
/// something that did not set SO_REUSEADDR and SO_REUSEPORT is already bound to the `address:port`.
pub fn new_socket(address: String, port: i32, backlog: Option<i32>) -> Result<socket2::Socket, Box<dyn Error>> {
    let address: SocketAddr = format!("{}:{}", address, port).parse()?;
    let (domain, ip_version) = socket_domain(address);
    tracing::trace!(address = %address, ip_version, "shared socket listening");
    let socket = socket2::Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    // Set value for the `SO_REUSEPORT` and `SO_REUSEADDR` options on this socket.
    // This indicates that further calls to `bind` may allow reuse of local
    // addresses. For IPv4 sockets this means that a socket may bind even when
    // there's a socket already listening on this port.
    socket.set_reuse_port(true)?;
    socket.set_reuse_address(true)?;
    socket.bind(&address.into())?;
    socket.listen(backlog.unwrap_or(1024))?;
    Ok(socket)
}

/// Find the socket domain
fn socket_domain(address: SocketAddr) -> (Domain, &'static str) {
    if address.is_ipv6() {
        (Domain::IPV6, "6")
    } else {
        (Domain::IPV4, "4")
    }
}
