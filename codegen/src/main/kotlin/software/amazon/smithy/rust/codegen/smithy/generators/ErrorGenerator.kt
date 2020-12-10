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
import software.amazon.smithy.rust.codegen.lang.rust
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
        val messageShape = shape.getMember("message")
        val message = messageShape.map { "self.message.as_deref()" }.orElse("None")
        writer.rustBlock("impl ${symbol.name}") {
            rust(
                """
            pub fn retryable(&self) -> bool { $retryable }
            pub fn throttling(&self) -> bool { $throttling }
            pub fn code(&self) -> &str { ${shape.id.name.dq()} }
            pub fn message(&self) -> Option<&str> { $message }
                """
            )
        }

        writer.rustBlock("impl #T for ${symbol.name}", StdFmt("Display")) {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                write("write!(f, ${symbol.name.dq()})?;")
                messageShape.map {
                    OptionForEach(symbolProvider.toSymbol(it), "&self.message") { field ->
                        write("""write!(f, ": {}", $field)?;""")
                    }
                }
                write("Ok(())")
            }
        }

        writer.write("impl #T for ${symbol.name} {}", StdError)
    }
}
