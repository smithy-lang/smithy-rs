/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerEnumGenerator
import software.amazon.smithy.rust.codegen.smithy.CodegenMode
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.docWithNote
import software.amazon.smithy.rust.codegen.util.getTrait

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
    override var mode: CodegenMode = CodegenMode.Server

    private val errorStruct = "${enumName}UnknownVariantError"
    private val codegenScope = arrayOf(
        "pyo3" to PythonServerCargoDependency.PyO3.asType(),
        "SmithyPython" to PythonServerCargoDependency.SmithyHttpServerPython(runtimeConfig).asType()
    )

    override fun renderFromForStr() {
        writer.rustTemplate(
            """
            ##[#{pyo3}::pyclass]
            ##[derive(Debug, PartialEq, Eq, Hash)]
            pub struct $errorStruct(String);
            """,
            *codegenScope
        )
        renderEnumImpl()
    }

    override fun renderEnum() {
        val renamedWarning =
            sortedMembers.mapNotNull { it.name() }.filter { it.renamedFrom != null }.joinToString("\n") {
                val previousName = it.renamedFrom!!
                "`$enumName::$previousName` has been renamed to `::${it.name}`."
            }
        writer.docWithNote(
            shape.getTrait<DocumentationTrait>()?.value,
            renamedWarning.ifBlank { null }
        )

        writer.rustTemplate("##[#{pyo3}::pyclass]", *codegenScope)
        meta.render(writer)
        writer.rustBlock("enum $enumName") {
            sortedMembers.forEach { member -> member.render(writer) }
            if (mode == CodegenMode.Client) {
                docs("$UnknownVariant contains new variants that have been added since this code was generated.")
                write("$UnknownVariant(String)")
            }
        }
        renderPyO3Methods(writer)
    }

    private fun renderPyO3Methods(writer: RustWriter) {
        writer.rustTemplate(
            """/// Python methods implementation for `$enumName`
            ##[#{pyo3}::pymethods]""",
            *codegenScope
        )
        writer.rustBlock("impl $enumName") {
            writer.rustTemplate(
                """
                fn __repr__(&self) -> String  {
                    self.as_str().to_owned()
                }
                fn __str__(&self) -> String {
                    self.as_str().to_owned()
                }
                """,
                *codegenScope
            )
        }
    }
}
