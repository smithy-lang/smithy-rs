/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.error

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.RetryableTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.errorMetadata
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.transformers.eventStreamErrors
import software.amazon.smithy.rust.codegen.core.smithy.transformers.operationErrors
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

/**
 * Generates a unified error enum for [operation]. [ErrorGenerator] handles generating the individual variants,
 * but we must still combine those variants into an enum covering all possible errors for a given operation.
 *
 * This generator also generates errors for event streams.
 */
class OperationErrorGenerator(
    private val model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val operationOrEventStream: Shape,
    private val customizations: List<ErrorCustomization>,
) {
    private val runtimeConfig = symbolProvider.config.runtimeConfig
    private val errorMetadata = errorMetadata(symbolProvider.config.runtimeConfig)
    private val createUnhandledError =
        RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::result::CreateUnhandledError")

    private fun operationErrors(): List<StructureShape> =
        (operationOrEventStream as OperationShape).operationErrors(model).map { it.asStructureShape().get() }
    private fun eventStreamErrors(): List<StructureShape> =
        (operationOrEventStream as UnionShape).eventStreamErrors()
            .map { model.expectShape(it.asMemberShape().get().target, StructureShape::class.java) }

    fun render(writer: RustWriter) {
        val (errorSymbol, errors) = when (operationOrEventStream) {
            is OperationShape -> symbolProvider.symbolForOperationError(operationOrEventStream) to operationErrors()
            is UnionShape -> symbolProvider.symbolForEventStreamError(operationOrEventStream) to eventStreamErrors()
            else -> UNREACHABLE("OperationErrorGenerator only supports operation or event stream shapes")
        }

        val meta = RustMetadata(
            derives = setOf(RuntimeType.Debug),
            additionalAttributes = listOf(Attribute.NonExhaustive),
            visibility = Visibility.PUBLIC,
        )

        writer.rust("/// Error type for the `${errorSymbol.name}` operation.")
        meta.render(writer)
        writer.rustBlock("enum ${errorSymbol.name}") {
            errors.forEach { errorVariant ->
                documentShape(errorVariant, model)
                deprecatedShape(errorVariant)
                val errorVariantSymbol = symbolProvider.toSymbol(errorVariant)
                write("${errorVariantSymbol.name}(#T),", errorVariantSymbol)
            }
            rustTemplate(
                """
                /// An unexpected error occurred (e.g., invalid JSON returned by the service or an unknown error code).
                #{deprecation}
                Unhandled(#{Unhandled}),
                """,
                "deprecation" to writable { renderUnhandledErrorDeprecation(runtimeConfig, errorSymbol.name) },
                "Unhandled" to unhandledError(runtimeConfig),
            )
        }

        writer.renderImpl(errorSymbol, errors)
        writer.renderImplStdError(errorSymbol, errors)
        writer.renderImplDisplay(errorSymbol, errors)
        writer.renderImplProvideErrorKind(errorSymbol, errors)
        writer.renderImplProvideErrorMetadata(errorSymbol, errors)
        writer.renderImplCreateUnhandledError(errorSymbol)
        writer.writeCustomizations(customizations, ErrorSection.OperationErrorAdditionalTraitImpls(errorSymbol, errors))
    }

    private fun RustWriter.renderImplCreateUnhandledError(errorSymbol: Symbol) {
        rustBlock("impl #T for ${errorSymbol.name}", createUnhandledError) {
            rustBlockTemplate(
                """
                fn create_unhandled_error(
                    source: #{Box}<dyn #{StdError} + #{Send} + #{Sync} + 'static>,
                    meta: #{Option}<#{ErrorMeta}>
                ) -> Self
                """,
                *preludeScope,
                "StdError" to RuntimeType.StdError,
                "ErrorMeta" to errorMetadata,
            ) {
                rustTemplate(
                    """Self::Unhandled(#{Unhandled} { source, meta: meta.unwrap_or_default() })""",
                    "Unhandled" to unhandledError(runtimeConfig),
                )
            }
        }
    }

    private fun RustWriter.renderImplDisplay(errorSymbol: Symbol, errors: List<StructureShape>) {
        rustBlock("impl #T for ${errorSymbol.name}", RuntimeType.Display) {
            rustBlock("fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result") {
                delegateToVariants(errors) { variantMatch ->
                    when (variantMatch) {
                        is VariantMatch.Unhandled -> writable {
                            rustTemplate(
                                """
                                if let #{Some}(code) = #{ProvideErrorMetadata}::code(self) {
                                    write!(f, "unhandled error ({code})")
                                } else {
                                    f.write_str("unhandled error")
                                }
                                """,
                                *preludeScope,
                                "ProvideErrorMetadata" to RuntimeType.provideErrorMetadataTrait(runtimeConfig),
                            )
                        }
                        is VariantMatch.Modeled -> writable { rust("_inner.fmt(f)") }
                    }
                }
            }
        }
    }

    private fun RustWriter.renderImplProvideErrorMetadata(errorSymbol: Symbol, errors: List<StructureShape>) {
        val errorMetadataTrait = RuntimeType.provideErrorMetadataTrait(runtimeConfig)
        rustBlock("impl #T for ${errorSymbol.name}", errorMetadataTrait) {
            rustBlock("fn meta(&self) -> &#T", errorMetadata(runtimeConfig)) {
                delegateToVariants(errors) { variantMatch ->
                    writable {
                        when (variantMatch) {
                            is VariantMatch.Unhandled -> rust("&_inner.meta")
                            is VariantMatch.Modeled -> rust("#T::meta(_inner)", errorMetadataTrait)
                        }
                    }
                }
            }
        }
    }

    private fun RustWriter.renderImplProvideErrorKind(errorSymbol: Symbol, errors: List<StructureShape>) {
        val retryErrorKindT = RuntimeType.retryErrorKind(symbolProvider.config.runtimeConfig)
        rustBlock(
            "impl #T for ${errorSymbol.name}",
            RuntimeType.provideErrorKind(symbolProvider.config.runtimeConfig),
        ) {
            rustBlockTemplate("fn code(&self) -> #{Option}<&str>", *preludeScope) {
                rust("#T::code(self)", RuntimeType.provideErrorMetadataTrait(runtimeConfig))
            }

            rustBlockTemplate(
                "fn retryable_error_kind(&self) -> #{Option}<#{ErrorKind}>",
                "ErrorKind" to retryErrorKindT,
                *preludeScope,
            ) {
                val retryableVariants = errors.filter { it.hasTrait<RetryableTrait>() }
                if (retryableVariants.isEmpty()) {
                    rustTemplate("#{None}", *preludeScope)
                } else {
                    rustBlock("match self") {
                        retryableVariants.forEach {
                            val errorVariantSymbol = symbolProvider.toSymbol(it)
                            rustTemplate(
                                "Self::${errorVariantSymbol.name}(inner) => #{Some}(inner.retryable_error_kind()),",
                                *preludeScope,
                            )
                        }
                        rustTemplate("_ => #{None}", *preludeScope)
                    }
                }
            }
        }
    }

    private fun RustWriter.renderImpl(errorSymbol: Symbol, errors: List<StructureShape>) {
        rustBlock("impl ${errorSymbol.name}") {
            rustTemplate(
                """
                /// Creates the `${errorSymbol.name}::Unhandled` variant from any error type.
                pub fn unhandled(err: impl #{Into}<#{Box}<dyn #{StdError} + #{Send} + #{Sync} + 'static>>) -> Self {
                    Self::Unhandled(#{Unhandled} { source: err.into(), meta: #{Default}::default() })
                }

                /// Creates the `${errorSymbol.name}::Unhandled` variant from an [`ErrorMetadata`](#{ErrorMetadata}).
                pub fn generic(err: #{ErrorMetadata}) -> Self {
                    Self::Unhandled(#{Unhandled} { source: err.clone().into(), meta: err })
                }
                """,
                *preludeScope,
                "ErrorMetadata" to errorMetadata,
                "StdError" to RuntimeType.StdError,
                "Unhandled" to unhandledError(runtimeConfig),
            )
            docs(
                """
                Returns error metadata, which includes the error code, message,
                request ID, and potentially additional information.
                """,
            )
            rustBlock("pub fn meta(&self) -> &#T", errorMetadata) {
                rustBlock("match self") {
                    errors.forEach { error ->
                        val errorVariantSymbol = symbolProvider.toSymbol(error)
                        rustTemplate(
                            "Self::${errorVariantSymbol.name}(e) => #{ProvideErrorMetadata}::meta(e),",
                            "ProvideErrorMetadata" to RuntimeType.provideErrorMetadataTrait(runtimeConfig),
                        )
                    }
                    rust("Self::Unhandled(e) => &e.meta,")
                }
            }
            errors.forEach { error ->
                val errorVariantSymbol = symbolProvider.toSymbol(error)
                val fnName = errorVariantSymbol.name.toSnakeCase()
                rust("/// Returns `true` if the error kind is `${errorSymbol.name}::${errorVariantSymbol.name}`.")
                rustBlock("pub fn is_$fnName(&self) -> bool") {
                    rust("matches!(self, Self::${errorVariantSymbol.name}(_))")
                }
            }
        }
    }

    private fun RustWriter.renderImplStdError(errorSymbol: Symbol, errors: List<StructureShape>) {
        rustBlock("impl #T for ${errorSymbol.name}", RuntimeType.StdError) {
            rustBlockTemplate(
                "fn source(&self) -> #{Option}<&(dyn #{StdError} + 'static)>",
                *preludeScope,
                "StdError" to RuntimeType.StdError,
            ) {
                delegateToVariants(errors) { variantMatch ->
                    when (variantMatch) {
                        is VariantMatch.Unhandled -> writable {
                            rustTemplate("#{Some}(&*_inner.source)", *preludeScope)
                        }
                        is VariantMatch.Modeled -> writable {
                            rustTemplate("#{Some}(_inner)", *preludeScope)
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
     *  match self {
     *      Self::InvalidGreeting(_inner) => inner.fmt(f),
     *      Self::ComplexError(_inner) => inner.fmt(f),
     *      Self::FooError(_inner) => inner.fmt(f),
     *      Self::Unhandled(_inner) => _inner.fmt(f),
     *  }
     *  ```
     *
     * [handler] is passed an instance of [VariantMatch]—a [writable] should be returned containing the content to be
     * written for this variant.
     *
     *  The field will always be bound as `_inner`.
     */
    fun RustWriter.delegateToVariants(
        errors: List<StructureShape>,
        handler: (VariantMatch) -> Writable,
    ) {
        rustBlock("match self") {
            errors.forEach {
                val errorSymbol = symbolProvider.toSymbol(it)
                rust("""Self::${errorSymbol.name}(_inner) => """)
                handler(VariantMatch.Modeled(errorSymbol, it))(this)
                write(",")
            }
            val unhandledHandler = handler(VariantMatch.Unhandled)
            rustBlock("Self::Unhandled(_inner) =>") {
                unhandledHandler(this)
            }
        }
    }
}
