/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.transformers.eventStreamErrors
import software.amazon.smithy.rust.codegen.core.smithy.transformers.operationErrors
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency

/**
 * Generates a unified error enum for [operation]. [ErrorGenerator] handles generating the individual variants,
 * but we must still combine those variants into an enum covering all possible errors for a given operation.
 */
open class ServerOperationErrorGenerator(
    private val model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val operationOrEventStream: Shape,
    private val schemaSerde: Boolean = false,
    runtimeConfig: RuntimeConfig? = null,
) {
    private val runtimeConfig =
        runtimeConfig ?: if (schemaSerde) {
            throw CodegenException("Schema operation errors require the codegen context's runtime configuration")
        } else {
            RuntimeConfig()
        }
    private val symbol = symbolProvider.toSymbol(operationOrEventStream)

    private fun operationErrors(): List<StructureShape> =
        (operationOrEventStream as OperationShape).operationErrors(model).map { it.asStructureShape().get() }

    private fun eventStreamErrors(): List<StructureShape> =
        (operationOrEventStream as UnionShape).eventStreamErrors()
            .map { model.expectShape(it.asMemberShape().get().target, StructureShape::class.java) }

    open fun render(writer: RustWriter) {
        val (errorSymbol, errors) =
            when (operationOrEventStream) {
                is OperationShape -> symbolProvider.symbolForOperationError(operationOrEventStream) to operationErrors()
                is UnionShape -> symbolProvider.symbolForEventStreamError(operationOrEventStream) to eventStreamErrors()
                else -> UNREACHABLE("OperationErrorGenerator only supports operation or event stream shapes")
            }
        if (errors.isEmpty()) {
            return
        }
        val meta =
            RustMetadata(
                derives = setOf(RuntimeType.Debug),
                visibility = Visibility.PUBLIC,
            )

        writer.rust("/// Error type for the `${symbol.name}` operation.")
        writer.rust("/// Each variant represents an error that can occur for the `${symbol.name}` operation.")
        meta.render(writer)
        writer.rustBlock("enum ${errorSymbol.name}") {
            errors.forEach { errorVariant ->
                documentShape(errorVariant, model)
                deprecatedShape(errorVariant)
                val errorVariantSymbol = symbolProvider.toSymbol(errorVariant)
                write("${errorVariantSymbol.name}(#T),", errorVariantSymbol)
            }
        }

        writer.rustBlock("impl #T for ${errorSymbol.name}", RuntimeType.Display) {
            rustBlock("fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result") {
                delegateToVariants(errors, errorSymbol) {
                    rust("_inner.fmt(f)")
                }
            }
        }

        writer.rustBlock("impl ${errorSymbol.name}") {
            errors.forEach { error ->
                val errorVariantSymbol = symbolProvider.toSymbol(error)
                val fnName = errorVariantSymbol.name.toSnakeCase()
                writer.rust("/// Returns `true` if the error kind is `${errorSymbol.name}::${errorVariantSymbol.name}`.")
                writer.rustBlock("pub fn is_$fnName(&self) -> bool") {
                    rust("matches!(&self, ${errorSymbol.name}::${errorVariantSymbol.name}(_))")
                }
            }
            writer.rust("/// Returns the error name string by matching the correct variant.")
            writer.rustBlock("pub fn name(&self) -> &'static str") {
                delegateToVariants(errors, errorSymbol) {
                    rust("_inner.name()")
                }
            }
        }

        writer.rustBlock("impl #T for ${errorSymbol.name}", RuntimeType.StdError) {
            rustBlock("fn source(&self) -> std::option::Option<&(dyn #T + 'static)>", RuntimeType.StdError) {
                delegateToVariants(errors, errorSymbol) {
                    rust("Some(_inner)")
                }
            }
        }

        if (schemaSerde && operationOrEventStream is OperationShape) {
            val operationNames = ServerOperationNames(symbol)
            val schema = RuntimeType.smithySchema(runtimeConfig)
            val serializable = schema.resolve("serde::SerializableStruct")
            val server = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
            val httpModeledError = server.resolve("schema::HttpModeledError")
            writer.rustBlock("impl #T for ${errorSymbol.name}", serializable) {
                rustBlock("fn schema(&self) -> &#T<'_>", schema.resolve("Schema")) {
                    delegateToVariants(errors, errorSymbol) { rust("#T::schema(_inner)", serializable) }
                }

                rustBlock("fn serialize_members(&self, serializer: &mut dyn #T) -> #T<(), #T>", schema.resolve("serde::ShapeSerializer"), RuntimeType.std.resolve("result::Result"), schema.resolve("serde::SerdeError")) {
                    delegateToVariants(errors, errorSymbol) { rust("#T::serialize_members(_inner, serializer)", serializable) }
                }
            }
            writer.rustBlock("impl #T for ${errorSymbol.name}", httpModeledError) {
                rustBlock("fn status_code(&self) -> u16") {
                    delegateToVariants(errors, errorSymbol) { rust("#T::status_code(_inner)", httpModeledError) }
                }
                rustTemplate(
                    """
                    fn operation_schema(&self) -> #{Option}<&#{Server}::schema::OperationSchema<'_>> {
                        #{Some}(<crate::operation_shape::${operationNames.markerType} as #{Server}::operation::SchemaOperationShape>::SCHEMA)
                    }
                    """,
                    *RuntimeType.preludeScope,
                    "Server" to server,
                )
            }
            writer.rustTemplate(
                """
                impl<P> #{IntoResponse}<P> for ${errorSymbol.name} {
                    fn into_response(self) -> #{Response} {
                        let mut response = #{Response}::new(#{Empty}());
                        *response.status_mut() = #{Http}::StatusCode::from_u16(#{HttpModeledError}::status_code(&self))
                            .expect("modeled error status is valid");
                        response.extensions_mut().insert(#{ModeledErrorExtension}::new(
                            #{InternHeaderName}(#{SerializableStruct}::schema(&self).shape_id().shape_name())));
                        response
                    }
                    fn into_response_with_protocol(self, protocol: &dyn #{ServerProtocol}) -> #{Response} {
                        protocol.serialize_error(&self)
                    }
                }
                """,
                "IntoResponse" to server.resolve("response::IntoResponse"),
                "Response" to server.resolve("response::Response"),
                "Empty" to server.resolve("body::empty"),
                "Http" to RuntimeType.http(runtimeConfig),
                "HttpModeledError" to server.resolve("schema::HttpModeledError"),
                "ModeledErrorExtension" to server.resolve("extension::ModeledErrorExtension"),
                "SerializableStruct" to RuntimeType.smithySchema(runtimeConfig).resolve("serde::SerializableStruct"),
                "ServerProtocol" to server.resolve("schema::ServerProtocol"),
                "InternHeaderName" to RuntimeType.smithySchema(runtimeConfig).resolve("intern_header_name"),
            )
        }

        for (error in errors) {
            val errorVariantSymbol = symbolProvider.toSymbol(error)
            writer.rustBlock("impl #T<#T> for #T", RuntimeType.From, errorVariantSymbol, errorSymbol) {
                rustBlock("fn from(variant: #T) -> #T", errorVariantSymbol, errorSymbol) {
                    rust("Self::${errorVariantSymbol.name}(variant)")
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
        errors: List<StructureShape>,
        symbol: Symbol,
        writable: Writable,
    ) {
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
