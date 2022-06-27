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
mod logging;
mod server;
mod socket;
mod state;
pub mod types;

#[doc(inline)]
pub use error::Error;
#[doc(inline)]
pub use logging::{setup, LogLevel};
#[doc(inline)]
pub use server::{PyApp, PyRouter};
#[doc(inline)]
pub use socket::SharedSocket;
#[doc(inline)]
pub use state::{PyHandler, PyHandlers, PyState};

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
