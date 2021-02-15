/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.RetryableTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.Derives
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.customize.Section

/**
 * For a given Operation ([this]), return the symbol referring to the unified error? This can be used
 * if you, eg. want to return a unfied error from a function:
 *
 * ```kotlin
 * rustWriter.rustBlock("fn get_error() -> #T", operation.errorSymbol(symbolProvider)) {
 *   write("todo!() // function body")
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
    model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val operation: OperationShape
) {

    private val operationIndex = OperationIndex.of(model)
    fun render(writer: RustWriter) {
        val errors = operationIndex.getErrors(operation)
        val symbol = operation.errorSymbol(symbolProvider)
        val meta = RustMetadata(
            derives = Derives(setOf(RuntimeType.StdFmt("Debug"))),
            additionalAttributes = listOf(Attribute.NonExhaustive),
            public = true
        )
        meta.render(writer)
        writer.rustBlock("enum ${symbol.name}") {
            errors.forEach { errorVariant ->
                val errorSymbol = symbolProvider.toSymbol(errorVariant)
                write("${errorSymbol.name}(#T),", errorSymbol)
            }
            rust(
                """
                /// An unexpected error, eg. invalid JSON returned by the service
                Unhandled(Box<dyn #T>),
            """,
                RuntimeType.StdError
            )
        }
        writer.rustBlock("impl #T for ${symbol.name}", RuntimeType.StdFmt("Display")) {
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

            rustBlock("fn error_kind(&self) -> Option<#T>", errorKindT) {
                delegateToVariants {
                    when (it) {
                        is VariantMatch.Modeled -> writable {
                            if (it.shape.hasTrait(RetryableTrait::class.java)) {
                                rust("Some(_inner.error_kind())")
                            } else {
                                rust("None")
                            }
                        }
                        is VariantMatch.Generic -> writable { rust("_inner.error_kind()") }
                        is VariantMatch.Unhandled -> writable { rust("None") }
                    }
                }
            }
        }

        writer.rustBlock("impl ${symbol.name}") {
            writer.rustBlock("pub fn unhandled<E: Into<Box<dyn #T>>>(err: E) -> Self", RuntimeType.StdError) {
                write("${symbol.name}::Unhandled(err.into())")
            }

            // Consider if this should actually be `Option<Cow<&str>>`. This would enable us to use display as implemented
            // by std::Error to generate a message in that case.
            writer.rustBlock("pub fn message(&self) -> Option<&str>") {
                delegateToVariants {
                    when (it) {
                        is VariantMatch.Generic, is VariantMatch.Modeled -> writable { rust("_inner.message()") }
                        else -> writable { rust("None") }
                    }
                }
            }

            writer.rustBlock("pub fn code(&self) -> Option<&str>") {
                delegateToVariants {
                    when (it) {
                        is VariantMatch.Unhandled -> writable { rust("None") }
                        is VariantMatch.Modeled -> writable { rust("Some(_inner.code())") }
                        is VariantMatch.Generic -> writable { rust("_inner.code()") }
                    }
                }
            }
        }

        writer.rustBlock("impl #T for ${symbol.name}", RuntimeType.StdError) {
            rustBlock("fn source(&self) -> Option<&(dyn #T + 'static)>", RuntimeType.StdError) {
                delegateToVariants {
                    writable {
                        when (it) {
                            is VariantMatch.Unhandled -> rust("Some(_inner.as_ref())")
                            is VariantMatch.Generic, is VariantMatch.Modeled -> rust("Some(_inner)")
                        }
                    }
                }
            }
        }
    }

    sealed class VariantMatch(name: String) : Section(name) {
        object Unhandled : VariantMatch("Unhandled")
        object Generic : VariantMatch("Generic")
        data class Modeled(val symbol: Symbol, val shape: Shape) : VariantMatch("Modeled")
    }

    /**
     * Generates code to delegate behavior to the variants, for example:
     * ```rust
     *  match self {
     *    GreetingWithErrorsError::InvalidGreeting(_inner) => inner.fmt(f),
     *    GreetingWithErrorsError::ComplexError(_inner) => inner.fmt(f),
     *    GreetingWithErrorsError::FooError(_inner) => inner.fmt(f),
     *    GreetingWithErrorsError::Unhandled(_inner) => match inner.downcast_ref::<::smithy_types::Error>() {
     *      Some(_inner) => inner.message(),
     *      None => None,
     *    }
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
        val genericError = RuntimeType.GenericError(symbolProvider.config().runtimeConfig)
        rustBlock("match self") {
            errors.forEach {
                val errorSymbol = symbolProvider.toSymbol(it)
                rust("""${symbol.name}::${errorSymbol.name}(_inner) => """)
                handler(VariantMatch.Modeled(errorSymbol, it))(this)
                write(",")
            }
            val genericHandler = handler(VariantMatch.Generic)
            val unhandledHandler = handler(VariantMatch.Unhandled)
            rustBlock("${symbol.name}::Unhandled(_inner) =>") {
                if (genericHandler != unhandledHandler) {
                    rustBlock("match _inner.downcast_ref::<#T>()", genericError) {
                        rustBlock("Some(_inner) => ") {
                            genericHandler(this)
                        }
                        rustBlock("None => ") {
                            unhandledHandler(this)
                        }
                    }
                } else {
                    // If the handlers are the same, skip the downcast
                    genericHandler(this)
                }
            }
        }
    }
}
