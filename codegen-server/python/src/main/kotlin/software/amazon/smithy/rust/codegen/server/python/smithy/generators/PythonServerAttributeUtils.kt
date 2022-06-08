/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency

/**
 * This module contains utilities to render PyO3 attributes.
 */

private val codegenScope = arrayOf(
    "pyo3" to PythonServerCargoDependency.PyO3.asType(),
)

/**
 * Renders #[pyo3::pyclass] attribute.
 */
fun RustWriter.renderPyClass() {
    rustTemplate("##[#{pyo3}::pyclass]", *codegenScope)
}

/**
 * Renders #[pyo3::pymethods] attribute.
 */
fun RustWriter.renderPyMethods() {
    rustTemplate("##[#{pyo3}::pymethods]", *codegenScope)
}
