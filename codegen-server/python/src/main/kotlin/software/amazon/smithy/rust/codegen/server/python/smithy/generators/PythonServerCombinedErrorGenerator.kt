/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerCombinedErrorGenerator
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol

/**
 * Generates a unified error enum for [operation]. It depends on [ServerCombinedErrorGenerator]
 * to generate the errors from the model and adds the Rust implementation `From<pyo3::PyErr>`.
 */
class PythonServerCombinedErrorGenerator(
    model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val operation: OperationShape
) : ServerCombinedErrorGenerator(model, symbolProvider, operation) {

    override fun render(writer: RustWriter) {
        super.render(writer)
        writer.rustTemplate(
            """
            impl #{From}<#{pyo3}::PyErr> for #{Error} {
                fn from(variant: #{pyo3}::PyErr) -> #{Error} {
                    crate::error::InternalServerError {
                        message: variant.to_string()
                    }.into()
                }
            }
            """,
            "pyo3" to PythonServerCargoDependency.PyO3.asType(),
            "Error" to operation.errorSymbol(symbolProvider),
            "From" to RuntimeType.From
        )
    }
}
