/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Rust/Python bindings, runtime and utilities.
//!
//! This crates implements all the generic code needed to start and manage
//! a Smithy Rust HTTP server where the business logic is implemented in Python,
//! leveraging [PyO3].
//!
//! [PyO3]: https://pyo3.rs/

mod error;
pub mod logging;
mod middleware;
mod server;
mod socket;
pub mod types;

#[doc(inline)]
pub use error::Error;
#[doc(inline)]
pub use logging::LogLevel;
#[doc(inline)]
pub use middleware::{
    py_middleware_wrapper, PyMiddleware, PyMiddlewareException, PyMiddlewareHandler,
    PyMiddlewareLayer, PyRequest,
};
#[doc(inline)]
pub use server::{PyApp, PyHandler};
#[doc(inline)]
pub use socket::PySocket;

#[cfg(test)]
mod tests {
    use std::sync::Once;

    static INIT: Once = Once::new();

    pub(crate) fn initialize() {
        INIT.call_once(|| {
            pyo3::prepare_freethreaded_python();
        });
    }
}
