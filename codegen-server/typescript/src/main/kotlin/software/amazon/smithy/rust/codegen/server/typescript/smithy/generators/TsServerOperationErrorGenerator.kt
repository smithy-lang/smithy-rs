/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.typescript.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerOperationErrorGenerator
import software.amazon.smithy.rust.codegen.server.typescript.smithy.TsServerCargoDependency

/**
 * Generates a unified error enum for [operation]. It depends on [ServerOperationErrorGenerator]
 * to generate the errors from the model and adds the Rust implementation for `napi` error.
 */
class TsServerOperationErrorGenerator(
    private val model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val operation: OperationShape,
) : ServerOperationErrorGenerator(model, symbolProvider, symbolProvider.toSymbol(operation), listOf()) {

    private val operationIndex = OperationIndex.of(model)
    private val errors = operationIndex.getErrors(operation)

    override fun render(writer: RustWriter) {
        super.render(writer)
        renderFromTsErr(writer)
    }

    private fun renderFromTsErr(writer: RustWriter) {
        writer.rustTemplate(
            """
            impl #{From}<#{napi}::Error> for #{Error} {
                fn from(variant: #{napi}::Error) -> #{Error} {
                    // TODO(fill-me-with-an-issue): match the Ts error type and return the right one.
                    crate::error::InternalServerError { message: variant.to_string() }.into()
                }
            }

            """,
            "napi" to TsServerCargoDependency.Napi.toType(),
            "Error" to operation.errorSymbol(symbolProvider),
            "From" to RuntimeType.From,
        )
    }
}
