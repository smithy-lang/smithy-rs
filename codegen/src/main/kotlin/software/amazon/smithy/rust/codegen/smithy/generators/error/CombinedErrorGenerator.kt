/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators.error

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.RetryableTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.customize.Section
import software.amazon.smithy.rust.codegen.util.hasTrait
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
class CombinedErrorGenerator(
    private val model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val operation: OperationShape
) {
    private val operationIndex = OperationIndex.of(model)
    private val runtimeConfig = symbolProvider.config().runtimeConfig
    private val genericError = RuntimeType.GenericError(symbolProvider.config().runtimeConfig)

    fun render(writer: RustWriter) {
        val errors = operationIndex.getErrors(operation)
        val operationSymbol = symbolProvider.toSymbol(operation)
        val symbol = operation.errorSymbol(symbolProvider)
        val meta = RustMetadata(
            derives = Attribute.Derives(setOf(RuntimeType.Debug)),
            additionalAttributes = listOf(Attribute.NonExhaustive),
            visibility = Visibility.PUBLIC
        )

        writer.rust("/// Error type for the `${operationSymbol.name}` operation.")
        meta.render(writer)
        writer.rustBlock("struct ${symbol.name}") {
            rust(
                """
                /// Kind of error that occurred.
                pub kind: ${symbol.name}Kind,
                /// Additional metadata about the error, including error code, message, and request ID.
                pub (crate) meta: #T
                """,
                RuntimeType.GenericError(runtimeConfig)
            )
        }
        writer.rust("/// Types of errors that can occur for the `${operationSymbol.name}` operation.")
        meta.render(writer)
        writer.rustBlock("enum ${symbol.name}Kind") {
            errors.forEach { errorVariant ->
                documentShape(errorVariant, model)
                val errorSymbol = symbolProvider.toSymbol(errorVariant)
                write("${errorSymbol.name}(#T),", errorSymbol)
            }
            rust(
                """
                /// An unexpected error, e.g. invalid JSON returned by the service or an unknown error code
                Unhandled(Box<dyn #T + Send + Sync + 'static>),
                """,
                RuntimeType.StdError
            )
        }
        writer.rustBlock("impl #T for ${symbol.name}", RuntimeType.Display) {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                delegateToVariants {
                    writable { rust("_inner.fmt(f)") }
                }
            }
        }

        val errorKindT = RuntimeType.errorKind(symbolProvider.config().runtimeConfig)
        writer.rustBlock(
            "impl #T for ${symbol.name}",
            RuntimeType.provideErrorKind(symbolProvider.config().runtimeConfig)
        ) {
            rustBlock("fn code(&self) -> Option<&str>") {
                rust("${symbol.name}::code(self)")
            }

            rustBlock("fn retryable_error_kind(&self) -> Option<#T>", errorKindT) {
                val retryableVariants = errors.filter { it.hasTrait<RetryableTrait>() }
                if (retryableVariants.isEmpty()) {
                    rust("None")
                } else {
                    rustBlock("match &self.kind") {
                        retryableVariants.forEach {
                            val errorSymbol = symbolProvider.toSymbol(it)
                            rust("${symbol.name}Kind::${errorSymbol.name}(inner) => Some(inner.retryable_error_kind()),")
                        }
                        rust("_ => None")
                    }
                }
            }
        }

        writer.rustBlock("impl ${symbol.name}") {
            writer.rustTemplate(
                """
                /// Creates a new `${symbol.name}`.
                pub fn new(kind: ${symbol.name}Kind, meta: #{generic_error}) -> Self {
                    Self { kind, meta }
                }

                /// Creates the `${symbol.name}::Unhandled` variant from any error type.
                pub fn unhandled(err: impl Into<Box<dyn #{std_error} + Send + Sync + 'static>>) -> Self {
                    Self {
                        kind: ${symbol.name}Kind::Unhandled(err.into()),
                        meta: Default::default()
                    }
                }

                /// Creates the `${symbol.name}::Unhandled` variant from a `#{generic_error}`.
                pub fn generic(err: #{generic_error}) -> Self {
                    Self {
                        meta: err.clone(),
                        kind: ${symbol.name}Kind::Unhandled(err.into()),
                    }
                }

                /// Returns the error message if one is available.
                pub fn message(&self) -> Option<&str> {
                    self.meta.message()
                }

                /// Returns error metadata, which includes the error code, message,
                /// request ID, and potentially additional information.
                pub fn meta(&self) -> &#{generic_error} {
                    &self.meta
                }

                /// Returns the request ID if it's available.
                pub fn request_id(&self) -> Option<&str> {
                    self.meta.request_id()
                }

                /// Returns the error code if it's available.
                pub fn code(&self) -> Option<&str> {
                    self.meta.code()
                }
                """,
                "generic_error" to genericError, "std_error" to RuntimeType.StdError
            )
            errors.forEach { error ->
                val errorSymbol = symbolProvider.toSymbol(error)
                val fnName = errorSymbol.name.toSnakeCase()
                writer.rust("/// Returns `true` if the error kind is `${symbol.name}Kind::${errorSymbol.name}`.")
                writer.rustBlock("pub fn is_$fnName(&self) -> bool") {
                    rust("matches!(&self.kind, ${symbol.name}Kind::${errorSymbol.name}(_))")
                }
            }
        }

        writer.rustBlock("impl #T for ${symbol.name}", RuntimeType.StdError) {
            rustBlock("fn source(&self) -> Option<&(dyn #T + 'static)>", RuntimeType.StdError) {
                delegateToVariants {
                    writable {
                        when (it) {
                            is VariantMatch.Unhandled -> rust("Some(_inner.as_ref())")
                            is VariantMatch.Modeled -> rust("Some(_inner)")
                        }
                    }
                }
            }
        }
    }

    sealed class VariantMatch(name: String) : Section(name) {
        object Unhandled : VariantMatch("Unhandled")
        data class Modeled(val symbol: Symbol, val shape: Shape) : VariantMatch("Modeled")
    }

    /**
     * Generates code to delegate behavior to the variants, for example:
     *
     * ```rust
     *  match &self.kind {
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
        rustBlock("match &self.kind") {
            errors.forEach {
                val errorSymbol = symbolProvider.toSymbol(it)
                rust("""${symbol.name}Kind::${errorSymbol.name}(_inner) => """)
                handler(VariantMatch.Modeled(errorSymbol, it))(this)
                write(",")
            }
            val unhandledHandler = handler(VariantMatch.Unhandled)
            rustBlock("${symbol.name}Kind::Unhandled(_inner) =>") {
                unhandledHandler(this)
            }
        }
    }
}
