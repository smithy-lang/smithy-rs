/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.util.toSnakeCase

/**
 * Generates a unified error enum for [operation]. [ErrorGenerator] handles generating the individual variants,
 * but we must still combine those variants into an enum covering all possible errors for a given operation.
 */
open class ServerCombinedErrorGenerator(
    private val model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val operation: OperationShape
) {
    private val operationIndex = OperationIndex.of(model)

    open fun render(writer: RustWriter) {
        val errors = operationIndex.getErrors(operation)
        val operationSymbol = symbolProvider.toSymbol(operation)
        val symbol = operation.errorSymbol(symbolProvider)
        val meta = RustMetadata(
            derives = Attribute.Derives(setOf(RuntimeType.Debug)),
            visibility = Visibility.PUBLIC
        )

        writer.rust("/// Error type for the `${operationSymbol.name}` operation.")
        writer.rust("/// Each variant represents an error that can occur for the `${operationSymbol.name}` operation.")
        meta.render(writer)
        writer.rustBlock("enum ${symbol.name}") {
            errors.forEach { errorVariant ->
                documentShape(errorVariant, model)
                val errorVariantSymbol = symbolProvider.toSymbol(errorVariant)
                write("${errorVariantSymbol.name}(#T),", errorVariantSymbol)
            }
        }

        writer.rustBlock("impl #T for ${symbol.name}", RuntimeType.Display) {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                delegateToVariants {
                    rust("_inner.fmt(f)")
                }
            }
        }

        writer.rustBlock("impl ${symbol.name}") {
            errors.forEach { error ->
                val errorSymbol = symbolProvider.toSymbol(error)
                val fnName = errorSymbol.name.toSnakeCase()
                writer.rust("/// Returns `true` if the error kind is `${symbol.name}::${errorSymbol.name}`.")
                writer.rustBlock("pub fn is_$fnName(&self) -> bool") {
                    rust("matches!(&self, ${symbol.name}::${errorSymbol.name}(_))")
                }
            }
            writer.rust("/// Returns the error name string by matching the correct variant.")
            writer.rustBlock("pub fn name(&self) -> &'static str") {
                delegateToVariants {
                    rust("_inner.name()")
                }
            }
        }

        writer.rustBlock("impl #T for ${symbol.name}", RuntimeType.StdError) {
            rustBlock("fn source(&self) -> Option<&(dyn #T + 'static)>", RuntimeType.StdError) {
                delegateToVariants {
                    rust("Some(_inner)")
                }
            }
        }

        for (error in errors) {
            val errorSymbol = symbolProvider.toSymbol(error)
            writer.rustBlock("impl #T<#T> for #T", RuntimeType.From, errorSymbol, symbol) {
                rustBlock("fn from(variant: #T) -> #T", errorSymbol, symbol) {
                    rust("Self::${errorSymbol.name}(variant)")
                }
            }
        }
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
     * A [writable] is passed containing the content to be written for each variant.
     *
     *  The field will always be bound as `_inner`.
     */
    private fun RustWriter.delegateToVariants(
        writable: Writable
    ) {
        val errors = operationIndex.getErrors(operation)
        val symbol = operation.errorSymbol(symbolProvider)
        rustBlock("match &self") {
            errors.forEach {
                val errorSymbol = symbolProvider.toSymbol(it)
                rust("""${symbol.name}::${errorSymbol.name}(_inner) => """)
                writable(this)
                write(",")
            }
        }
    }
}
