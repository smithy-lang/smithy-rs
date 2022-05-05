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
        // TODO: Ipv6 support.
        let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
        let address: SocketAddr = format!("{}:{}", address, port).parse()?;
        tracing::info!("Socket listening on {}", address);
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

impl SharedSocket {
    pub fn get_socket(&self) -> Socket {
        self.socket.try_clone().unwrap()
    }
}
