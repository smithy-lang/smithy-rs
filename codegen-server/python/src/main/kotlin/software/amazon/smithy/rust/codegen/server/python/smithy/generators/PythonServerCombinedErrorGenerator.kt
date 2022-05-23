/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerCombinedErrorGenerator
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol

/**
 * Generates a unified error enum for [operation]. It depends on [ServerCombinedErrorGenerator]
 * to generate the errors from the model and adds the Rust implementation `From<pyo3::PyErr>`.
 */
class PythonServerCombinedErrorGenerator(
    private val model: Model,
    private val codegenContext: CodegenContext,
    private val operation: OperationShape
) : ServerCombinedErrorGenerator(model, codegenContext.symbolProvider, operation) {
    private val operationIndex = OperationIndex.of(model)

    override fun render(writer: RustWriter) {
        super.render(writer)
        val symbol = operation.errorSymbol(codegenContext.symbolProvider)
        val errorSymbol = PythonServerCargoDependency.PyO3.asType()
        writer.rustBlock("impl From<#T::PyErr> for #T", errorSymbol, symbol) {
            rustBlock("fn from(variant: #T::PyErr) -> #T", errorSymbol, symbol) {
                rust(
                    """InternalServerError {
                    message: variant.to_string(),
                        }.into()"""
                )
            }
        }
    }
}
