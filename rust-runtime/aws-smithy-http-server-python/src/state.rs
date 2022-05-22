/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! State object definition.
use std::{collections::HashMap, ops::Deref, sync::Arc};

use pyo3::prelude::*;

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

pub type PyHandlers = HashMap<String, Arc<PyHandler>>;

#[pyclass]
#[derive(Debug, Clone)]
pub struct State {
    pub context: Arc<PyObject>,
    pub handlers: PyHandlers,
}

impl State {
    fn new(context: PyObject, handlers: PyHandlers) -> Self {
        Self {
            context: Arc::new(context),
            handlers,
        }
    }
}
