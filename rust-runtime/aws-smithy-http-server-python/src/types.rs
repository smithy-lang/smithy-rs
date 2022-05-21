/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use pyo3::prelude::*;

#[pyclass]
#[derive(Debug, Clone, PartialEq)]
pub struct Blob(aws_smithy_types::Blob);

impl std::ops::Deref for Blob {
    type Target = aws_smithy_types::Blob;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
