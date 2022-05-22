/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerRuntimeType
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerCombinedErrorGenerator
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol

/**
 * Generates a unified error enum for [operation]. [ErrorGenerator] handles generating the
 * individual variants, but we must still combine those variants into an enum covering all possible
 * errors for a given operation.
 */
class PythonServerCombinedErrorGenerator(
    private val model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val operation: OperationShape,
    private val runtimeConfig: RuntimeConfig
) : ServerCombinedErrorGenerator(model, symbolProvider, operation) {
    private val operationIndex = OperationIndex.of(model)

    override fun render(writer: RustWriter) {
        super.render(writer)
        val symbol = operation.errorSymbol(symbolProvider)
        val errorSymbol = PythonServerRuntimeType.PyError(runtimeConfig)
        writer.rustBlock("impl From<#T> for #T", errorSymbol, symbol) {
            rustBlock("fn from(variant: #T) -> #T", errorSymbol, symbol) {
                rust(
                    """InternalServerError {
                    message: variant.to_string(),
                        }.into()"""
                )
            }
        }
    }
}
