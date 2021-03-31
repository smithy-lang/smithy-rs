/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators.error

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig

/**
 * Each service defines it's own "top-level" error combining all possible errors that a service can emit.
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
class TopLevelErrorGenerator(protocolConfig: ProtocolConfig, private val operations: List<OperationShape>) {
    private val symbolProvider = protocolConfig.symbolProvider
    private val model = protocolConfig.model

    private val allErrors = operations.flatMap { it.errors }.distinctBy { it.name }
        .map { protocolConfig.model.expectShape(it, StructureShape::class.java) }
        .sortedBy { it.id.name }

    private val sdkError = CargoDependency.SmithyHttp(protocolConfig.runtimeConfig).asType().member("result::SdkError")
    fun render(crate: RustCrate) {
        crate.withModule(RustModule.default("error_meta", false)) { writer ->
            writer.renderDefinition()
            writer.renderImplDisplay()
            // Every operation error can be converted into service::Error
            operations.forEach { operationShape ->
                writer.renderImplFrom(operationShape)
            }
            writer.rust("impl #T for Error {}", RuntimeType.StdError)
        }
        crate.lib { it.rust("pub use error_meta::Error;") }
    }

    private fun RustWriter.renderImplDisplay() {
        rustBlock("impl std::fmt::Display for Error") {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                // For now, just delegate to the debug implementation
                rust("""write!(f, "{:?}", &self)""")
            }
        }
    }


    private fun RustWriter.renderImplFrom(
        operationShape: OperationShape,
    ) {
        val operationError = operationShape.errorSymbol(symbolProvider)
        rustBlock("impl From<#T<#T>> for Error", sdkError, operationError) {
            rustBlock("fn from(err: #T<#T>) -> Self", sdkError, operationError) {
                rustBlock("match err") {
                    val operationErrors = operationShape.errors.map { model.expectShape(it) }
                    rustBlock("#T::ServiceError { err, ..} => match err.kind", sdkError) {
                        operationErrors.forEach { errorShape ->
                            val errSymbol = symbolProvider.toSymbol(errorShape)
                            rust("#TKind::${errSymbol.name}(inner) => Error::${errSymbol.name}(inner),", operationError)
                        }
                        rust("#TKind::Unhandled(inner) => Error::Unhandled(inner),", operationError)
                    }
                    rust("_ => Error::Unhandled(err.into()),")
                }
            }
        }
    }

    private fun RustWriter.renderDefinition() {
        RustMetadata(additionalAttributes = listOf(Attribute.NonExhaustive), public = true).withDerives(RuntimeType.Debug).render(this)
        rustBlock("enum Error") {
            allErrors.forEach { error ->
                val sym = symbolProvider.toSymbol(error)
                rust("${sym.name}(#T),", sym)
            }
            rust("Unhandled(Box<dyn #T>)", RuntimeType.StdError)
        }
    }
}
