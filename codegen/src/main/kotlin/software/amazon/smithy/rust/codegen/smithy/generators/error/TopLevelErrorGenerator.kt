/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators.error

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.generators.CodegenTarget
import software.amazon.smithy.rust.codegen.smithy.transformers.allErrors
import software.amazon.smithy.rust.codegen.smithy.transformers.eventStreamErrors

/**
 * Each service defines its own "top-level" error combining all possible errors that a service can emit.
 *
 * Every service error is convertible into this top level error, which enables (if desired) authoring a single error handling
 * path. Eg:
 * ```rust
 * // dynamodb/src/lib.rs
 * enum Error {
 *   ListTablesError(ListTablesError),
 *   ValidationError(ValidationError),
 *   ...,
 *   // It also includes cases from SdkError
 * }
 * ```
 */
class TopLevelErrorGenerator(private val coreCodegenContext: CoreCodegenContext, private val operations: List<OperationShape>) {
    private val symbolProvider = coreCodegenContext.symbolProvider
    private val model = coreCodegenContext.model

    private val allErrors = operations.flatMap { it.allErrors(model) }.map { it.id }.distinctBy { it.getName(coreCodegenContext.serviceShape) }
        .map { coreCodegenContext.model.expectShape(it, StructureShape::class.java) }
        .sortedBy { it.id.getName(coreCodegenContext.serviceShape) }

    private val sdkError = CargoDependency.SmithyHttp(coreCodegenContext.runtimeConfig).asType().member("result::SdkError")
    fun render(crate: RustCrate) {
        crate.withModule(RustModule.default("error_meta", visibility = Visibility.PRIVATE)) { writer ->
            writer.renderDefinition()
            writer.renderImplDisplay()
            // Every operation error can be converted into service::Error
            operations.forEach { operationShape ->
                // operation errors
                writer.renderImplFrom(operationShape.errorSymbol(model, symbolProvider, coreCodegenContext.target), operationShape.errors)
            }
            // event stream errors
            operations.map { it.eventStreamErrors(coreCodegenContext.model) }
                .flatMap { it.entries }
                .associate { it.key to it.value }
                .forEach { (unionShape, errors) ->
                    writer.renderImplFrom(
                        unionShape.eventStreamErrorSymbol(
                            model,
                            symbolProvider,
                            coreCodegenContext.target,
                        ),
                        errors.map { it.id },
                    )
                }
            writer.rust("impl #T for Error {}", RuntimeType.StdError)
        }
        crate.lib { it.rust("pub use error_meta::Error;") }
    }

    private fun RustWriter.renderImplDisplay() {
        rustBlock("impl #T for Error", RuntimeType.Display) {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                rustBlock("match self") {
                    allErrors.forEach {
                        rust("Error::${symbolProvider.toSymbol(it).name}(inner) => inner.fmt(f),")
                    }
                    rust("Error::Unhandled(inner) => inner.fmt(f)")
                }
            }
        }
    }

    private fun RustWriter.renderImplFrom(symbol: RuntimeType, errors: List<ShapeId>) {
        if (errors.isNotEmpty() || CodegenTarget.CLIENT == coreCodegenContext.target) {
            rustBlock(
                "impl<R> From<#T<#T, R>> for Error where R: Send + Sync + std::fmt::Debug + 'static",
                sdkError,
                symbol,
            ) {
                rustBlockTemplate(
                    "fn from(err: #{SdkError}<#{OpError}, R>) -> Self",
                    "SdkError" to sdkError,
                    "OpError" to symbol,
                ) {
                    rustBlock("match err") {
                        val operationErrors = errors.map { model.expectShape(it) }
                        rustBlock("#T::ServiceError { err, ..} => match err.kind", sdkError) {
                            operationErrors.forEach { errorShape ->
                                val errSymbol = symbolProvider.toSymbol(errorShape)
                                rust(
                                    "#TKind::${errSymbol.name}(inner) => Error::${errSymbol.name}(inner),",
                                    symbol,
                                )
                            }
                            rust("#TKind::Unhandled(inner) => Error::Unhandled(inner),", symbol)
                        }
                        rust("_ => Error::Unhandled(err.into()),")
                    }
                }
            }
        }
    }

    private fun RustWriter.renderDefinition() {
        rust("/// All possible error types for this service.")
        RustMetadata(
            additionalAttributes = listOf(Attribute.NonExhaustive),
            visibility = Visibility.PUBLIC,
        ).withDerives(RuntimeType.Debug).render(this)
        rustBlock("enum Error") {
            allErrors.forEach { error ->
                documentShape(error, model)
                val sym = symbolProvider.toSymbol(error)
                rust("${sym.name}(#T),", sym)
            }
            rust("/// An unhandled error occurred.")
            rust("Unhandled(Box<dyn #T + Send + Sync + 'static>)", RuntimeType.StdError)
        }
    }
}
