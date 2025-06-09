/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
/* End of automatically managed default lints */
#![allow(clippy::derive_partial_eq_without_eq)]
#![cfg_attr(docsrs, feature(doc_cfg))]

//! Rust/Python bindings, runtime and utilities.
//!
//! This crates implements all the generic code needed to start and manage
//! a Smithy Rust HTTP server where the business logic is implemented in Python,
//! leveraging [PyO3].
//!
//! [PyO3]: https://pyo3.rs/

pub mod context;
mod error;
pub mod lambda;
pub mod logging;
pub mod middleware;
mod server;
mod socket;
pub mod tls;
pub mod types;
mod util;

#[doc(inline)]
pub use error::{PyError, PyMiddlewareException};
#[doc(inline)]
pub use logging::{py_tracing_event, PyTracingHandler};
#[doc(inline)]
pub use middleware::{PyMiddlewareHandler, PyMiddlewareLayer, PyRequest, PyResponse};
#[doc(inline)]
pub use server::{PyApp, PyHandler};
#[doc(inline)]
pub use socket::PySocket;
#[doc(inline)]
pub use util::error::{rich_py_err, RichPyErr};

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use pyo3::{types::PyAnyMethods, PyErr, Python};
    use pyo3_async_runtimes::TaskLocals;

    static INIT: Once = Once::new();

    pub(crate) fn initialize() -> TaskLocals {
        INIT.call_once(|| {
            pyo3::prepare_freethreaded_python();
        });

        Python::with_gil(|py| {
            let asyncio = py.import("asyncio")?;
            let event_loop = asyncio.call_method0("new_event_loop")?;
            asyncio.call_method1("set_event_loop", (&event_loop,))?;
            Ok::<TaskLocals, PyErr>(TaskLocals::new(event_loop))
        })
        .unwrap()
    }
}
