/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerEnumGenerator
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider

/**
 * To share enums defined in Rust with Python, `pyo3` provides the `PyClass` trait.
 * This class generates enums definitions, implements the `PyClass` trait and adds
 * some utility functions like `__str__()` and `__repr__()`.
 */
class PythonServerEnumGenerator(
    model: Model,
    symbolProvider: RustSymbolProvider,
    private val writer: RustWriter,
    private val shape: StringShape,
    enumTrait: EnumTrait,
    runtimeConfig: RuntimeConfig,
) : ServerEnumGenerator(model, symbolProvider, writer, shape, enumTrait, runtimeConfig) {

    private val pyo3Symbols = listOf(PythonServerCargoDependency.PyO3.asType())

    override fun render() {
        renderPyClass()
        super.render()
        renderPyO3Methods()
    }

    private fun renderPyClass() {
        Attribute.Custom("pyo3::pyclass", symbols = pyo3Symbols).render(writer)
    }

    override fun renderFromForStr() {
        renderPyClass()
        super.renderFromForStr()
    }

    private fun renderPyO3Methods() {
        Attribute.Custom("pyo3::pymethods", symbols = pyo3Symbols).render(writer)

        writer.rust(
            """
            impl $enumName {
                fn __repr__(&self) -> String  {
                    self.as_str().to_owned()
                }
                fn __str__(&self) -> String {
                    self.as_str().to_owned()
                }
            }
            """
        )
    }
}
