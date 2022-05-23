/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Python server trait.
//!
//! This file contains the [aws_smithy_http_server_python::PyServer] trait implementation
//! and common functionalities, such as handling workers life-cycle.
use std::{sync::Arc, thread};

use aws_smithy_http_server::{AddExtensionLayer, Router};
use pyo3::prelude::*;
use pyo3::IntoPy;
use pyo3_asyncio::TaskLocals;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

use crate::{state::PyHandlers, PyHandler, SharedSocket, State};

/// Trait implementing the common functionalities of the Rust Python server.
pub trait PyServer: Clone {
    /// Mutable reference to `handlers`, used to register new `PyHandlers`.
    fn handlers_mut(&mut self) -> &mut PyHandlers;
    /// Set [pyo3_asyncio::TaskLocals] (update the event loop).
    fn locals(&mut self, locals: TaskLocals);
    /// Register the context object in the `State`.
    fn context(&self, py: Python) -> Arc<PyObject>;
    /// Start a single python worker.
    fn start_single_python_worker(&self, py: Python) -> PyResult<PyObject>;
    /// Generate a `Router`.
    fn app(&self) -> Router;

    /// Register a new operation in the handlers map.
    fn add_operation(&mut self, py: Python, name: &str, func: PyObject) -> PyResult<()> {
        let inspect = py.import("inspect")?;
        // Check if the function is a coroutine.
        // NOTE: that `asyncio.iscoroutine()` doesn't work here.
        let is_coroutine = inspect
            .call_method1("iscoroutinefunction", (&func,))?
            .extract::<bool>()?;
        // Find number of expected methods (a Python implementation could not accept the context).
        let func_args = inspect
            .call_method1("getargs", (func.getattr(py, "__code__")?,))?
            .getattr("args")?
            .extract::<Vec<String>>()?;
        let func = PyHandler {
            func,
            is_coroutine,
            args: func_args.len(),
        };
        tracing::info!(
            "Registering {} function `{}` for operation {} with {} arguments",
            if func.is_coroutine { "async" } else { "sync" },
            name,
            func.func,
            func.args
        );
        // Insert the handler in the handlers map.
        self.handlers_mut()
            .insert(String::from(name), Arc::new(func));
        Ok(())
    }

    /// Start a signle worker with its own Tokio and Python async runtime
    /// and using the provided shared socket.
    fn start_single_worker(
        &'static mut self,
        py: Python,
        socket: &PyCell<SharedSocket>,
        worker_number: isize,
    ) -> PyResult<()>
    where
        Self: Send,
    {
        tracing::info!("Starting Rust Python server worker {}", worker_number);
        // Clone the socket.
        let borrow = socket.try_borrow_mut()?;
        let held_socket: &SharedSocket = &*borrow;
        let raw_socket = held_socket.get_socket()?;
        // Setup the Python asyncio loop to use `uvloop`.
        let asyncio = py.import("asyncio")?;
        let uvloop = py.import("uvloop")?;
        uvloop.call_method0("install")?;
        tracing::debug!("Setting up uvloop for current process");
        let event_loop = asyncio.call_method0("new_event_loop")?;
        asyncio.call_method1("set_event_loop", (event_loop,))?;
        // Create `State` object from the Python context object.
        let state = State::new(self.context(py));

        tracing::debug!("Start the Tokio runtime in a background task");
        // Store Python event loop locals.
        self.locals(pyo3_asyncio::TaskLocals::new(event_loop));
        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .thread_name(format!("smithy-rs[{}]", worker_number))
                .build()
                .unwrap();
            // Register operations into a Router, add middleware and start the `hyper` server.
            rt.block_on(async move {
                tracing::debug!("Add middlewares to Rust Python router");
                let app = self.app().layer(
                    ServiceBuilder::new()
                        .layer(TraceLayer::new_for_http())
                        .layer(AddExtensionLayer::new(state)),
                );
                tracing::debug!("Starting hyper server from shared socket");
                let server = hyper::Server::from_tcp(raw_socket.try_into().unwrap())
                    .unwrap()
                    .serve(app.into_make_service());
                // Run forever-ish...

                if let Err(err) = server.await {
                    tracing::error!("server error: {}", err);
                }
            });
        });
        // Block on the event loop forever.
        let event_loop = (*event_loop).call_method0("run_forever");
        tracing::debug!("Run and block on the Python event loop");
        tracing::info!("Rust Python server started successfully");
        if event_loop.is_err() {
            tracing::warn!("Ctrl-c handler, quitting");
        }
        Ok(())
    }

    /// Start the server on multiple workers.
    fn start_server(
        &mut self,
        py: Python,
        address: Option<String>,
        port: Option<i32>,
        backlog: Option<i32>,
        workers: Option<usize>,
    ) -> PyResult<()> {
        let mp = py.import("multiprocessing")?;
        mp.call_method0("allow_connection_pickling")?;
        let address = address.unwrap_or_else(|| String::from("127.0.0.1"));
        let port = port.unwrap_or(8080);
        let socket = SharedSocket::new(address, port, backlog)?;
        for idx in 0..workers.unwrap_or_else(num_cpus::get) {
            let sock = socket.try_clone()?;
            let process = mp.getattr("Process")?;
            let handle = process.call1((
                py.None(),
                self.start_single_python_worker(py)?,
                format!("smithy-rs[{}]", idx),
                (sock.into_py(py), idx),
            ))?;
            handle.call_method0("start")?;
        }
        Ok(())
    }
}
