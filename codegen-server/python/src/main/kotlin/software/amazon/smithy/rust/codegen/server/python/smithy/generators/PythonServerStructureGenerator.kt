/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.hasTrait

/**
 * To share structures defined in Rust with Python, `pyo3` provides the `PyClass` trait.
 * This class generates input / output / error structures definitions and implements the
 * `PyClass` trait.
 */
open class PythonServerStructureGenerator(
    model: Model,
    private val codegenContext: CodegenContext,
    private val symbolProvider: RustSymbolProvider,
    private val writer: RustWriter,
    private val shape: StructureShape
) : StructureGenerator(model, symbolProvider, writer, shape) {
    private val codegenScope =
        arrayOf(
            "pyo3" to PythonServerCargoDependency.PyO3.asType(),
            "SmithyPython" to PythonServerCargoDependency.SmithyHttpServerPython(codegenContext.runtimeConfig).asType()
        )

    override fun renderStructure() {
        val symbol = symbolProvider.toSymbol(shape)
        val containerMeta = symbol.expectRustMetadata()
        if (shape.hasTrait<ErrorTrait>()) {
            writer.rustTemplate("##[#{pyo3}::pyclass(extends = pyo3::exceptions::PyException)]", *codegenScope)
        } else {
            writer.rustTemplate("##[#{pyo3}::pyclass]", *codegenScope)
        }
        writer.documentShape(shape, model)
        val withoutDebug = containerMeta.derives.copy(
            derives = containerMeta.derives.derives - RuntimeType.Debug
        )
        containerMeta.copy(derives = withoutDebug).render(writer)

        writer.rustBlock("struct $name ${lifetimeDeclaration()}") {
            forEachMember(members) { member, memberName, memberSymbol ->
                renderMemberDoc(member, memberSymbol)
                writer.rustTemplate("##[#{pyo3}(get, set)]", *codegenScope)
                memberSymbol.expectRustMetadata().render(this)
                write("$memberName: #T,", symbolProvider.toSymbol(member))
            }
        }

        renderStructureImpl()
        renderDebugImpl()
        renderPyO3Methods()
    }

    private fun renderPyO3Methods() {
        if (shape.hasTrait<ErrorTrait>() || !accessorMembers.isEmpty()) {
            writer.rustTemplate(
                """/// Python methods implementation for `$name`
                ##[#{pyo3}::pymethods]""",
                *codegenScope
            )
            writer.rustBlock("impl $name") {
                write(
                    """##[new]
                    /// Create a new `$name` that can be instantiated by Python.
                    pub fn new("""
                )
                // Render field accessor methods
                forEachMember(accessorMembers) { _, memberName, memberSymbol ->
                    val memberType = memberSymbol.rustType()
                    write("$memberName: ${memberType.render()},")
                }
                if (shape.hasTrait<ErrorTrait>()) {
                    write("message: String,")
                }
                rustBlock(") -> Self") {
                    rustBlock("Self") {
                        forEachMember(accessorMembers) { _, memberName, _ -> write("$memberName,") }
                        if (shape.hasTrait<ErrorTrait>()) {
                            write("message,")
                        }
                    }
                }
                rustTemplate(
                    """
                    fn __repr__(&self) -> String  {
                        format!("{self:?}")
                    }
                    fn __str__(&self) -> String {
                        self.0.to_string()
                    }

                    """,
                    *codegenScope
                )
            }
        }
    }
}
