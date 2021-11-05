/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.customize.Section
import software.amazon.smithy.rust.codegen.util.toSnakeCase

/**
 * For a given Operation ([this]), return the symbol referring to the unified error. This can be used
 * if you, e.g. want to return a unified error from a function:
 *
 * ```kotlin
 * rustWriter.rustBlock("fn get_error() -> #T", operation.errorSymbol(symbolProvider)) {
 *     write("todo!() // function body")
 * }
 * ```
 */
fun OperationShape.errorSymbol(symbolProvider: SymbolProvider): RuntimeType {
    val symbol = symbolProvider.toSymbol(this)
    return RuntimeType("${symbol.name}Error", null, "crate::error")
}

/**
 * Generates a unified error enum for [operation]. [ErrorGenerator] handles generating the individual variants,
 * but we must still combine those variants into an enum covering all possible errors for a given operation.
 */
class ServerCombinedErrorGenerator(
    private val model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val operation: OperationShape
) {
    private val operationIndex = OperationIndex.of(model)

    fun render(writer: RustWriter) {
        // TODO Don't generate an enum error if the operation does not have any errors registered (Healthcheck).
        val errors = operationIndex.getErrors(operation)
        val operationSymbol = symbolProvider.toSymbol(operation)
        val symbol = operation.errorSymbol(symbolProvider)
        val meta = RustMetadata(
            derives = Attribute.Derives(setOf(RuntimeType.Debug)),
            public = true
        )

        writer.rust("/// Error type for the `${operationSymbol.name}` operation.")
        writer.rust("/// Each variant represents an error that can occur for the `${operationSymbol.name}` operation.")
        meta.render(writer)
        writer.rustBlock("enum ${symbol.name}Kind") {
            errors.forEach { errorVariant ->
                documentShape(errorVariant, model)
                val errorVariantSymbol = symbolProvider.toSymbol(errorVariant)
                write("${errorVariantSymbol.name}(#T),", errorVariantSymbol)
            }
        }

        writer.rustBlock("impl #T for ${symbol.name}Kind", RuntimeType.stdfmt.member("Display")) {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                delegateToVariants {
                    writable { rust("_inner.fmt(f)") }
                }
            }
        }

        writer.rustBlock("impl ${symbol.name}Kind") {
            // TODO This generates empty impl block if errors is empty
            errors.forEach { error ->
                val errorSymbol = symbolProvider.toSymbol(error)
                val fnName = errorSymbol.name.toSnakeCase()
                writer.rust("/// Returns `true` if the error kind is `${symbol.name}::${errorSymbol.name}`.")
                writer.rustBlock("pub fn is_$fnName(&self) -> bool") {
                    rust("matches!(&self, ${symbol.name}::${errorSymbol.name}(_))")
                }
            }
        }

        writer.rustBlock("impl #T for ${symbol.name}Kind", RuntimeType.StdError) {
            rustBlock("fn source(&self) -> Option<&(dyn #T + 'static)>", RuntimeType.StdError) {
                delegateToVariants {
                    writable {
                        when (it) {
                            is VariantMatch.Modeled -> rust("Some(_inner)")
                        }
                    }
                }
            }
        }
    }

    sealed class VariantMatch(name: String) : Section(name) {
        data class Modeled(val symbol: Symbol, val shape: Shape) : VariantMatch("Modeled")
    }

    /**
     * Generates code to delegate behavior to the variants, for example:
     *
     * ```rust
     *  match &self {
     *      GreetingWithErrorsError::InvalidGreeting(_inner) => inner.fmt(f),
     *      GreetingWithErrorsError::ComplexError(_inner) => inner.fmt(f),
     *      GreetingWithErrorsError::FooError(_inner) => inner.fmt(f),
     *      GreetingWithErrorsError::Unhandled(_inner) => _inner.fmt(f),
     *  }
     *  ```
     *
     * [handler] is passed an instance of [VariantMatch]—a [writable] should be returned containing the content to be
     * written for this variant.
     *
     *  The field will always be bound as `_inner`.
     */
    private fun RustWriter.delegateToVariants(
        handler: (VariantMatch) -> Writable
    ) {
        val errors = operationIndex.getErrors(operation)
        val symbol = operation.errorSymbol(symbolProvider)
        rustBlock("match &self") {
            errors.forEach {
                val errorSymbol = symbolProvider.toSymbol(it)
                rust("""${symbol.name}Kind::${errorSymbol.name}(_inner) => """)
                handler(VariantMatch.Modeled(errorSymbol, it))(this)
                write(",")
            }
        }
    }
}
