/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerEnumGenerator
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.util.dq

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

    private val codegenScope = arrayOf(
        "pyo3" to PythonServerCargoDependency.PyO3.asType(),
    )

    override fun render() {
        writer.renderPyClass(shape)
        super.render()
        renderPyO3Methods()
    }

    override fun renderFromForStr() {
        writer.renderPyClass(shape)
        super.renderFromForStr()
    }

    private fun renderPyO3Methods() {
        writer.renderPyMethods()
        writer.rustTemplate(
            """
            impl $enumName {
                #{name_method:W}
                ##[getter]
                pub fn value(&self) -> &str {
                    self.as_str()
                }
                fn __repr__(&self) -> String  {
                    self.as_str().to_owned()
                }
                fn __str__(&self) -> String {
                    self.as_str().to_owned()
                }
            }
            """,
            *codegenScope,
            "name_method" to renderPyEnumName()
        )
    }

    private fun renderPyEnumName(): Writable =
        writable {
            rustBlockTemplate(
                """
                ##[getter]
                pub fn name(&self) -> &str
                """,
                *codegenScope
            ) {
                rustBlock("match self") {
                    sortedMembers.forEach { member ->
                        val memberName = member.name()?.name
                        rust("""$enumName::$memberName => ${memberName?.dq()},""")
                    }
                }
            }
        }
}
