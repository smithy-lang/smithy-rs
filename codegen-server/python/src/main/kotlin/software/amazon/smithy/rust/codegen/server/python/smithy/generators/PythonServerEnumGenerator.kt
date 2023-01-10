/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerEnumGenerator

/**
 * To share enums defined in Rust with Python, `pyo3` provides the `PyClass` trait.
 * This class generates enums definitions, implements the `PyClass` trait and adds
 * some utility functions like `__str__()` and `__repr__()`.
 */
class PythonServerEnumGenerator(
    codegenContext: ServerCodegenContext,
    private val writer: RustWriter,
    shape: StringShape,
) : ServerEnumGenerator(codegenContext, writer, shape) {

    private val pyO3 = PythonServerCargoDependency.PyO3.toType()

    override fun render() {
        renderPyClass()
        super.render()
        renderPyO3Methods()
    }

    private fun renderPyClass() {
        Attribute(pyO3.resolve("pyclass")).render(writer)
    }

    private fun renderPyO3Methods() {
        Attribute(pyO3.resolve("pymethods")).render(writer)
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
            "name_method" to renderPyEnumName(),
        )
    }

    private fun renderPyEnumName(): Writable =
        writable {
            rustBlock(
                """
                ##[getter]
                pub fn name(&self) -> &str
                """,
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
