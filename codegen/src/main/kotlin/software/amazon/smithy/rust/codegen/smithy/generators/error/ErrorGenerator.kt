/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators.error

import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.RetryableTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RuntimeType.Companion.StdError
import software.amazon.smithy.rust.codegen.smithy.RuntimeType.Companion.stdfmt
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.smithy.transformers.errorMessageMember
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.getTrait

sealed class ErrorKind {
    abstract fun writable(runtimeConfig: RuntimeConfig): Writable

    object Throttling : ErrorKind() {
        override fun writable(runtimeConfig: RuntimeConfig) =
            writable { rust("#T::ThrottlingError", RuntimeType.errorKind(runtimeConfig)) }
    }

    object Client : ErrorKind() {
        override fun writable(runtimeConfig: RuntimeConfig) =
            writable { rust("#T::ClientError", RuntimeType.errorKind(runtimeConfig)) }
    }

    object Server : ErrorKind() {
        override fun writable(runtimeConfig: RuntimeConfig) =
            writable { rust("#T::ServerError", RuntimeType.errorKind(runtimeConfig)) }
    }
}

/**
 * Returns the modeled retryKind for this shape
 *
 * This is _only_ non-null in cases where the @retryable trait has been applied.
 */
fun StructureShape.modeledRetryKind(errorTrait: ErrorTrait): ErrorKind? {
    val retryableTrait = this.getTrait<RetryableTrait>() ?: return null
    return when {
        retryableTrait.throttling -> ErrorKind.Throttling
        errorTrait.isClientError -> ErrorKind.Client
        errorTrait.isServerError -> ErrorKind.Server
        // The error _must_ be either a client or server error
        else -> TODO()
    }
}

class ErrorGenerator(
    private val symbolProvider: RustSymbolProvider,
    private val writer: RustWriter,
    private val shape: StructureShape,
    private val error: ErrorTrait
) {
    fun render() {
        renderError()
    }

    private fun renderError() {
        val symbol = symbolProvider.toSymbol(shape)
        val messageShape = shape.errorMessageMember()
        val message = messageShape?.let { "self.${symbolProvider.toMemberName(it)}.as_deref()" } ?: "None"
        val errorKindT = RuntimeType.errorKind(symbolProvider.config().runtimeConfig)
        writer.rustBlock("impl ${symbol.name}") {
            val retryKindWriteable = shape.modeledRetryKind(error)?.writable(symbolProvider.config().runtimeConfig)
            if (retryKindWriteable != null) {
                rust("/// Returns `Some(${errorKindT.name})` if the error is retryable. Otherwise, returns `None`.")
                rustBlock("pub fn retryable_error_kind(&self) -> #T", errorKindT) {
                    retryKindWriteable(this)
                }
            }
            rust(
                """
                /// Returns the error message.
                pub fn message(&self) -> Option<&str> { $message }

                ##[doc(hidden)]
                /// Returns the error name.
                pub fn name(&self) -> String {
                    String::from("${shape.id.name}")
                }
                """
            )
        }

        writer.rustBlock("impl #T for ${symbol.name}", stdfmt.member("Display")) {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                // If the error id and the Rust name don't match, print the actual error id for easy debugging
                // Note: Exceptions cannot be renamed so it is OK to not call `getName(service)` here
                val errorDesc = symbol.name.letIf(symbol.name != shape.id.name) { symbolName ->
                    "$symbolName [${shape.id.name}]"
                }
                write("write!(f, ${errorDesc.dq()})?;")
                messageShape?.let {
                    ifSet(it, symbolProvider.toSymbol(it), "&self.message") { field ->
                        write("""write!(f, ": {}", $field)?;""")
                    }
                }
                write("Ok(())")
            }
        }
        writer.write("impl #T for ${symbol.name} {}", StdError)
    }
}
