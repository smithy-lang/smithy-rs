/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Schedule pure-Python middlewares as [tower::Layer]s.
//!
//! # Moving data from Rust to Python and back
//!
//! In middlewares we need to move some data back-and-forth between Rust and Python.
//! When you move some data from Rust to Python you can't get its ownership back,
//! you can only get `&T` or `&mut T` but not `T` unless you clone it.
//!
//! In order to overcome this shortcoming we are using wrappers for Python that holds
//! pure-Rust types with [Option]s and provides `take_inner(&mut self) -> Option<T>`
//! method to get the ownership of `T` back.
//!
//! For example:
//! ```no_run
//! # use pyo3::prelude::*;
//! # use pyo3::exceptions::PyRuntimeError;
//! # enum PyMiddlewareError {
//! #     InnerGone
//! # }
//! # impl From<PyMiddlewareError> for PyErr {
//! #     fn from(_: PyMiddlewareError) -> PyErr {
//! #         PyRuntimeError::new_err("inner gone")
//! #     }
//! # }
//! // Pure Rust type
//! struct Inner {
//!     num: i32
//! }
//!
//! // Python wrapper
//! #[pyclass]
//! pub struct Wrapper(Option<Inner>);
//!
//! impl Wrapper {
//!     // Call when Python is done processing the `Wrapper`
//!     // to get ownership of `Inner` back
//!     pub fn take_inner(&mut self) -> Option<Inner> {
//!         self.0.take()
//!     }
//! }
//!
//! // Python exposed methods checks if `Wrapper` still has the `Inner` and
//! // fails with `InnerGone` otherwise.
//! #[pymethods]
//! impl Wrapper {
//!     #[getter]
//!     fn num(&self) -> PyResult<i32> {
//!         self.0
//!             .as_ref()
//!             .map(|inner| inner.num)
//!             .ok_or_else(|| PyMiddlewareError::InnerGone.into())
//!     }
//!
//!     #[setter]
//!     fn set_num(&mut self, num: i32) -> PyResult<()> {
//!         match self.0.as_mut() {
//!             Some(inner) => {
//!                 inner.num = num;
//!                 Ok(())
//!             }
//!             None => Err(PyMiddlewareError::InnerGone.into()),
//!         }
//!     }
//! }
//! ```
//!
//! You can see this pattern in [PyRequest], [PyResponse] and the others.
//!

mod error;
mod handler;
mod header_map;
mod layer;
mod request;
mod response;

pub use self::error::PyMiddlewareError;
pub use self::handler::PyMiddlewareHandler;
pub use self::header_map::PyHeaderMap;
pub use self::layer::PyMiddlewareLayer;
pub use self::request::PyRequest;
pub use self::response::PyResponse;
