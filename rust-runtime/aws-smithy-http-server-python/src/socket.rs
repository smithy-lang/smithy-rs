/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Socket implementation that can be shared between multiple Python processes.
//!
//! Python cannot handle true multi-threaded applications due to the [GIL],
//! often resulting in reduced performance and only one core used by the application.
//! To work around this, Python web applications usually create a socket with
//! SO_REUSEADDR and SO_REUSEPORT enabled that can be shared between multiple
//! Python processes, allowing to maximize performance and use all available
//! computing capacity of the host.
//!
//! [GIL]: https://wiki.python.org/moin/GlobalInterpreterLock
use aws_smithy_http_server::socket::new_socket;
use pyo3::{exceptions::PyIOError, prelude::*};

#[pyclass]
#[derive(Debug)]
pub struct PySocket(socket2::Socket);

#[pymethods]
impl PySocket {
    /// Create a new UNIX `SharedSocket` from an address, port and backlog.
    /// If not specified, the backlog defaults to 1024 connections.
    #[new]
    pub fn new(address: String, port: i32, backlog: Option<i32>) -> PyResult<Self> {
        Ok(Self(
            new_socket(address, port, backlog).map_err(|e| PyIOError::new_err(e.to_string()))?,
        ))
    }

    /// Clone the inner socket allowing it to be shared between multiple
    /// Python processes.
    #[pyo3(text_signature = "($self)")]
    pub fn try_clone(&self) -> PyResult<PySocket> {
        Ok(PySocket(
            self.0
                .try_clone()
                .map_err(|e| PyIOError::new_err(e.to_string()))?,
        ))
    }
}

impl PySocket {
    pub fn to_raw_socket(&self) -> PyResult<socket2::Socket> {
        Ok(self
            .0
            .try_clone()
            .map_err(|e| PyIOError::new_err(e.to_string()))?)
    }
}
