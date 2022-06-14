/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.util.hasTrait

/**
 * This module contains utilities to render PyO3 attributes.
 *
 * TODO(https://github.com/awslabs/smithy-rs/issues/1465): Switch to `Attribute.Custom` and get rid of this class.
 */

private val codegenScope = arrayOf(
    "pyo3" to PythonServerCargoDependency.PyO3.asType(),
)

// Renders #[pyo3::pyclass] attribute, inheriting from `Exception` if the shape has the `ErrorTrait` attached.
fun RustWriter.renderPyClass(shape: Shape) {
    if (shape.hasTrait<ErrorTrait>()) {
        rustTemplate("##[#{pyo3}::pyclass(extends = #{pyo3}::exceptions::PyException)]", *codegenScope)
    } else {
        rustTemplate("##[#{pyo3}::pyclass]", *codegenScope)
    }
}

// Renders #[pyo3::pymethods] attribute.
fun RustWriter.renderPyMethods() {
    rustTemplate("##[#{pyo3}::pymethods]", *codegenScope)
}

// Renders #[pyo3(get, set)] attribute.
fun RustWriter.renderPyGetterSetter() {
    rustTemplate("##[#{pyo3}(get, set)]", *codegenScope)
}
