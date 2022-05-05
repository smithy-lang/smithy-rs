/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use pyo3::prelude::*;

use socket2::{Domain, Protocol, Socket, Type};
use std::net::SocketAddr;

#[pyclass]
#[derive(Debug)]
pub struct SharedSocket {
    pub socket: Socket,
}

#[pymethods]
impl SharedSocket {
    #[new]
    pub fn new(address: String, port: i32, backlog: i32) -> PyResult<Self> {
        let address: SocketAddr = format!("{}:{}", address, port).parse()?;
        let domain = if address.is_ipv6() { Domain::IPV6 } else { Domain::IPV4 };
        tracing::info!("Socket listening on {address}, IP version: {domain:?}");
        let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
        socket.set_reuse_port(true)?;
        socket.set_reuse_address(true)?;
        socket.bind(&address.into())?;
        socket.listen(backlog)?;
        Ok(SharedSocket { socket })
    }

    pub fn try_clone(&self) -> PyResult<SharedSocket> {
        let copied = self.socket.try_clone()?;
        Ok(SharedSocket { socket: copied })
    }
}
