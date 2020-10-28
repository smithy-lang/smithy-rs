/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.RetryableTrait
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType.Companion.StdError
import software.amazon.smithy.rust.codegen.smithy.RuntimeType.Companion.StdFmt
import software.amazon.smithy.rust.codegen.util.dq

class ErrorGenerator(
    val model: Model,
    private val symbolProvider: SymbolProvider,
    private val writer: RustWriter,
    private val shape: StructureShape,
    private val error: ErrorTrait
) {
    fun render() {
        renderError()
    }

    private fun renderError() {
        val symbol = symbolProvider.toSymbol(shape)
        val retryableTrait = shape.getTrait(RetryableTrait::class.java)
        val throttling = retryableTrait.map { it.throttling }.orElse(false)
        val retryable = retryableTrait.isPresent
        val errorCause = when {
            error.isClientError -> "ErrorCause::Client"
            error.isServerError -> "ErrorCause::Server"
            else -> "ErrorCause::Unknown(${error.value.dq()})"
        }
        writer.rustBlock("impl ${symbol.name}") {
            write("// TODO: create shared runtime crate")
            write("// fn at_fault(&self) -> ErrorCause { $errorCause }")
            write("pub fn retryable(&self) -> bool { $retryable }")
            write("pub fn throttling(&self) -> bool { $throttling }")
        }

        writer.rustBlock("impl \$T for ${symbol.name}", StdFmt("Display")) {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                val message = shape.getMember("message")
                write("write!(f, ${symbol.name.dq()})?;")
                if (message.isPresent) {
                    OptionForEach(symbolProvider.toSymbol(message.get()), "&self.message") { field ->
                        write("""write!(f, ": {}", $field)?;""")
                    }
                }
                write("Ok(())")
            }
        }

        writer.write("impl \$T for ${symbol.name} {}", StdError)
    }
}
