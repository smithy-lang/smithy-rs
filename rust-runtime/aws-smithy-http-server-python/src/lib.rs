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

mod logging;
mod socket;

#[doc(inline)]
pub use logging::{setup, LogLevel};
#[doc(inline)]
pub use socket::SharedSocket;
