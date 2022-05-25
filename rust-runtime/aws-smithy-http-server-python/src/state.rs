/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [PyState] and Python handlers..
use std::{collections::HashMap, ops::Deref, sync::Arc};

use pyo3::prelude::*;

/// The Python business logic implementation needs to carry some information
/// to be executed properly like the size of its arguments and if it is
/// a coroutine.
#[derive(Debug, Clone)]
pub struct PyHandler {
    pub func: PyObject,
    pub args: usize,
    pub is_coroutine: bool,
}

impl Deref for PyHandler {
    type Target = PyObject;

    fn deref(&self) -> &Self::Target {
        &self.func
    }
}

/// Mapping holding the Python business logic handlers.
pub type PyHandlers = HashMap<String, Arc<PyHandler>>;

/// [PyState] structure holding the Python context.
///
/// The possibility of passing the State or not is decided in Python if the method
/// `context()` is called on the `App` to register a context object.
#[pyclass]
#[derive(Debug, Clone)]
pub struct PyState {
    pub context: Arc<PyObject>,
}

impl PyState {
    /// Create a new [PyState] structure.
    pub fn new(context: Arc<PyObject>) -> Self {
        Self { context }
    }
}
