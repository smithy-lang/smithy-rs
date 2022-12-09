/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators.error

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.RetryableTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.transformers.eventStreamErrors
import software.amazon.smithy.rust.codegen.core.smithy.transformers.operationErrors
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

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
/*
    TODO(Error): refactor when core-codegen is available
    This is a possible future implementation:
    ```
    inline fun <reified T : CombinedErrorGenerator> OperationShape.errorSymbol(
        model: Model,
        symbolProvider: RustSymbolProvider,
        generator: KClass<T>
    ): RuntimeType {
        val symbol = symbolProvider.toSymbol(this)
        return RuntimeType.forInlineFun("${symbol.name}Error", RustModule.Error) {
                generator.java.newInstance().render(
                    this,
                    model,
                    symbolProvider,
                    symbol,
                    this.operationErrors(model).map { it.asStructureShape().get() }
                )
        }
    }
    ```
    Similarly for eventStreamErrorSymbol() below
 */
fun OperationShape.errorSymbol(
    model: Model,
    symbolProvider: RustSymbolProvider,
    target: CodegenTarget,
): RuntimeType {
    val symbol = symbolProvider.toSymbol(this)
    return RuntimeType.forInlineFun("${symbol.name}Error", RustModule.Error) {
        when (target) {
            CodegenTarget.CLIENT -> CombinedErrorGenerator(
                model,
                symbolProvider,
                symbol,
                operationErrors(model).map { it.asStructureShape().get() },
            ).render(this)
            CodegenTarget.SERVER -> ServerCombinedErrorGenerator(
                model,
                symbolProvider,
                symbol,
                operationErrors(model).map { it.asStructureShape().get() },
            ).render(this)
        }
    }
}

fun UnionShape.eventStreamErrorSymbol(model: Model, symbolProvider: RustSymbolProvider, target: CodegenTarget): RuntimeType {
    val symbol = symbolProvider.toSymbol(this)
    val errorSymbol = RuntimeType("crate::error::${symbol.name}Error")
    return RuntimeType.forInlineFun("${symbol.name}Error", RustModule.Error) {
        val errors = this@eventStreamErrorSymbol.eventStreamErrors().map { model.expectShape(it.asMemberShape().get().target, StructureShape::class.java) }
        when (target) {
            CodegenTarget.CLIENT ->
                CombinedErrorGenerator(model, symbolProvider, symbol, errors).renderErrors(
                    this,
                    errorSymbol,
                    symbol,
                )
            CodegenTarget.SERVER ->
                ServerCombinedErrorGenerator(model, symbolProvider, symbol, errors).renderErrors(
                    this,
                    errorSymbol,
                    symbol,
                )
        }
    }
}

/**
 * Generates a unified error enum for [operation]. [ErrorGenerator] handles generating the individual variants,
 * but we must still combine those variants into an enum covering all possible errors for a given operation.
 */
class CombinedErrorGenerator(
    private val model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val operationSymbol: Symbol,
    private val errors: List<StructureShape>,
) {
    private val runtimeConfig = symbolProvider.config().runtimeConfig
    private val genericError = RuntimeType.genericError(symbolProvider.config().runtimeConfig)
    private val createUnhandledError =
        RuntimeType.smithyHttp(runtimeConfig).resolve("result::CreateUnhandledError")

    fun render(writer: RustWriter) {
        val errorSymbol = RuntimeType("crate::error::${operationSymbol.name}Error")
        renderErrors(writer, errorSymbol, operationSymbol)
    }

    fun renderErrors(
        writer: RustWriter,
        errorSymbol: RuntimeType,
        operationSymbol: Symbol,
    ) {
        val meta = RustMetadata(
            derives = Attribute.Derives(setOf(RuntimeType.Debug)),
            additionalAttributes = listOf(Attribute.NonExhaustive),
            visibility = Visibility.PUBLIC,
        )

        writer.rust("/// Error type for the `${operationSymbol.name}` operation.")
        meta.render(writer)
        writer.rustBlock("struct ${errorSymbol.name}") {
            rust(
                """
                /// Kind of error that occurred.
                pub kind: ${errorSymbol.name}Kind,
                /// Additional metadata about the error, including error code, message, and request ID.
                pub (crate) meta: #T
                """,
                RuntimeType.genericError(runtimeConfig),
            )
        }
        writer.rustBlock("impl #T for ${errorSymbol.name}", createUnhandledError) {
            rustBlock("fn create_unhandled_error(source: Box<dyn std::error::Error + Send + Sync + 'static>) -> Self") {
                rustBlock("Self") {
                    rust("kind: ${errorSymbol.name}Kind::Unhandled(#T::new(source)),", unhandledError())
                    rust("meta: Default::default()")
                }
            }
        }

        writer.rust("/// Types of errors that can occur for the `${operationSymbol.name}` operation.")
        meta.render(writer)
        writer.rustBlock("enum ${errorSymbol.name}Kind") {
            errors.forEach { errorVariant ->
                documentShape(errorVariant, model)
                deprecatedShape(errorVariant)
                val errorVariantSymbol = symbolProvider.toSymbol(errorVariant)
                write("${errorVariantSymbol.name}(#T),", errorVariantSymbol)
            }
            docs(UNHANDLED_ERROR_DOCS)
            rust(
                """
                Unhandled(#T),
                """,
                unhandledError(),
            )
        }
        writer.rustBlock("impl #T for ${errorSymbol.name}", RuntimeType.Display) {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                delegateToVariants(errors, errorSymbol) {
                    writable { rust("_inner.fmt(f)") }
                }
            }
        }

        val errorKindT = RuntimeType.errorKind(symbolProvider.config().runtimeConfig)
        writer.rustBlock(
            "impl #T for ${errorSymbol.name}",
            RuntimeType.provideErrorKind(symbolProvider.config().runtimeConfig),
        ) {
            rustBlock("fn code(&self) -> Option<&str>") {
                rust("${errorSymbol.name}::code(self)")
            }

            rustBlock("fn retryable_error_kind(&self) -> Option<#T>", errorKindT) {
                val retryableVariants = errors.filter { it.hasTrait<RetryableTrait>() }
                if (retryableVariants.isEmpty()) {
                    rust("None")
                } else {
                    rustBlock("match &self.kind") {
                        retryableVariants.forEach {
                            val errorVariantSymbol = symbolProvider.toSymbol(it)
                            rust("${errorSymbol.name}Kind::${errorVariantSymbol.name}(inner) => Some(inner.retryable_error_kind()),")
                        }
                        rust("_ => None")
                    }
                }
            }
        }

        writer.rustBlock("impl ${errorSymbol.name}") {
            writer.rustTemplate(
                """
                /// Creates a new `${errorSymbol.name}`.
                pub fn new(kind: ${errorSymbol.name}Kind, meta: #{generic_error}) -> Self {
                    Self { kind, meta }
                }

                /// Creates the `${errorSymbol.name}::Unhandled` variant from any error type.
                pub fn unhandled(err: impl Into<Box<dyn #{std_error} + Send + Sync + 'static>>) -> Self {
                    Self {
                        kind: ${errorSymbol.name}Kind::Unhandled(#{Unhandled}::new(err.into())),
                        meta: Default::default()
                    }
                }

                /// Creates the `${errorSymbol.name}::Unhandled` variant from a `#{generic_error}`.
                pub fn generic(err: #{generic_error}) -> Self {
                    Self {
                        meta: err.clone(),
                        kind: ${errorSymbol.name}Kind::Unhandled(#{Unhandled}::new(err.into())),
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
                "generic_error" to genericError,
                "std_error" to RuntimeType.StdError,
                "Unhandled" to unhandledError(),
            )
            errors.forEach { error ->
                val errorVariantSymbol = symbolProvider.toSymbol(error)
                val fnName = errorVariantSymbol.name.toSnakeCase()
                writer.rust("/// Returns `true` if the error kind is `${errorSymbol.name}Kind::${errorVariantSymbol.name}`.")
                writer.rustBlock("pub fn is_$fnName(&self) -> bool") {
                    rust("matches!(&self.kind, ${errorSymbol.name}Kind::${errorVariantSymbol.name}(_))")
                }
            }
        }

        writer.rustBlock("impl #T for ${errorSymbol.name}", RuntimeType.StdError) {
            rustBlock("fn source(&self) -> Option<&(dyn #T + 'static)>", RuntimeType.StdError) {
                delegateToVariants(errors, errorSymbol) {
                    writable {
                        rust("Some(_inner)")
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
        errors: List<StructureShape>,
        symbol: RuntimeType,
        handler: (VariantMatch) -> Writable,
    ) {
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
