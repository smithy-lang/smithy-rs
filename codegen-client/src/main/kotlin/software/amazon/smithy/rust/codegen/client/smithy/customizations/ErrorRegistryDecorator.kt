/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.transformers.operationErrors

/**
 * Generates per-operation error registries (`mod error_registry`) and a service-wide
 * error registry (`crate::error_type_registry`) plus a `Client::error_registry()` accessor.
 *
 * Per the Document Type & Type Registries SEP § "Package level error type registry":
 * - The service-wide error registry contains every shape with the `@error` trait in
 *   the service closure. It is the lookup table for runtime error-discriminator dispatch
 *   and for third-party Document-based use cases (e.g., reifying a Document into a
 *   typed error variant via [`TypeRegistry::deserialize_document`]).
 * - The per-operation error registry contains only the errors that operation can throw,
 *   and lives inside the operation's module. Customers handling a `Document` known to
 *   contain one of an operation's error variants can use it for scoped dispatch.
 *
 * Only fires for services in [SchemaSerdeAllowlist]; non-allowlisted services pay no
 * artifact-size cost. Per-operation registries are only emitted for operations with
 * at least one modeled error to avoid empty registries.
 *
 * The pattern mirrors [TypeRegistryDecorator] except:
 * - The shape filter is inverted: the primary registry excludes errors; here errors
 *   are the *only* shapes included.
 * - The decorator emits both per-op modules (via [OperationSection.AdditionalItems])
 *   and a service-wide module (via [ClientCodegenDecorator.extras]).
 */
class ErrorRegistryDecorator : ClientCodegenDecorator {
    override val name: String = "ErrorRegistry"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        if (!SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
            return baseCustomizations
        }
        return baseCustomizations + ErrorRegistryOperationCustomization(codegenContext, operation)
    }

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        if (!SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
            return
        }

        val model = codegenContext.model
        val symbolProvider = codegenContext.symbolProvider
        val rc = codegenContext.runtimeConfig

        val smithySchema = RuntimeType.smithySchema(rc)
        val smithyTypes = RuntimeType.smithyTypes(rc)
        val typeRegistry = smithySchema.resolve("registry::TypeRegistry")
        val typeErasedBox = smithyTypes.resolve("type_erasure::TypeErasedBox")
        val shapeDeserializer = smithySchema.resolve("serde::ShapeDeserializer")
        val serdeError = smithySchema.resolve("serde::SerdeError")
        val lazyLock = RuntimeType.std.resolve("sync::LazyLock")

        // Walk the service closure for every @error-trait structure shape, sorted by
        // shape id for deterministic output.
        val errorShapes =
            DirectedWalker(model).walkShapes(codegenContext.serviceShape)
                .filterIsInstance<StructureShape>()
                .filter { it.hasTrait(ErrorTrait::class.java) }
                .sortedBy { it.id.toString() }

        // Emit `crate::error_type_registry` private module with the service-wide
        // LazyLock<TypeRegistry> static.
        val registryModule = RustModule.private("error_type_registry")
        rustCrate.withModule(registryModule) {
            rustTemplate(
                """
                pub(crate) static REGISTRY: #{LazyLock}<#{TypeRegistry}> = #{LazyLock}::new(|| {
                    #{TypeRegistry}::builder()
                        #{Entries}
                        .build()
                });
                """,
                "LazyLock" to lazyLock,
                "TypeRegistry" to typeRegistry,
                "Entries" to
                    writable {
                        errorShapes.forEach { shape ->
                            val type = symbolProvider.toSymbol(shape)
                            rustTemplate(
                                """
                                .insert_shape(
                                    #{Type}::SCHEMA,
                                    |d: &mut dyn #{ShapeDeserializer}| -> #{Result}<#{TypeErasedBox}, #{SerdeError}> {
                                        #{Result}::Ok(#{TypeErasedBox}::new(#{Type}::deserialize(d)?))
                                    },
                                )
                                """,
                                "Type" to type,
                                "ShapeDeserializer" to shapeDeserializer,
                                "TypeErasedBox" to typeErasedBox,
                                "SerdeError" to serdeError,
                                "Result" to RuntimeType.std.resolve("result::Result"),
                            )
                        }
                    },
            )
        }

        // Add `impl Client { pub fn error_registry() -> &'static TypeRegistry { ... } }`
        // alongside the Client struct definition in `crate::client`.
        rustCrate.withModule(ClientRustModule.client) {
            rustTemplate(
                """
                impl Client {
                    /// Returns the service-wide error registry.
                    ///
                    /// The registry contains an entry for every `@error`-trait structure shape
                    /// in the service closure, keyed by [`ShapeId`](#{ShapeId}). It supports
                    /// runtime error-discriminator dispatch and reifying a [`Document`](#{Document})
                    /// into a typed error via [`TypeRegistry::deserialize_document`](#{TypeRegistry}::deserialize_document).
                    ///
                    /// ```ignore
                    /// let registry = MyClient::error_registry();
                    /// let typed = registry.deserialize_document(&doc)?;
                    /// // Downcast to the concrete error variant you expected.
                    /// ```
                    pub fn error_registry() -> &'static #{TypeRegistry} {
                        &crate::error_type_registry::REGISTRY
                    }
                }
                """,
                "TypeRegistry" to typeRegistry,
                "ShapeId" to smithySchema.resolve("ShapeId"),
                "Document" to smithyTypes.resolve("Document"),
            )
        }
    }
}

/**
 * Emits a per-operation `mod error_registry { pub(crate) static REGISTRY: LazyLock<TypeRegistry> }`
 * inside the operation's module via [OperationSection.AdditionalItems].
 *
 * Operations with no modeled errors are skipped — no point in an empty registry.
 */
private class ErrorRegistryOperationCustomization(
    private val codegenContext: ClientCodegenContext,
    private val operation: OperationShape,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable =
        writable {
            if (section !is OperationSection.AdditionalItems) {
                return@writable
            }
            val errors =
                operation.operationErrors(codegenContext.model)
                    .filterIsInstance<StructureShape>()
                    .filter { it.hasTrait(ErrorTrait::class.java) }
                    .sortedBy { it.id.toString() }
            if (errors.isEmpty()) {
                return@writable
            }

            val rc = codegenContext.runtimeConfig
            val symbolProvider = codegenContext.symbolProvider
            val smithySchema = RuntimeType.smithySchema(rc)
            val smithyTypes = RuntimeType.smithyTypes(rc)
            val typeRegistry = smithySchema.resolve("registry::TypeRegistry")
            val typeErasedBox = smithyTypes.resolve("type_erasure::TypeErasedBox")
            val shapeDeserializer = smithySchema.resolve("serde::ShapeDeserializer")
            val serdeError = smithySchema.resolve("serde::SerdeError")
            val lazyLock = RuntimeType.std.resolve("sync::LazyLock")

            rustTemplate(
                """
                /// Per-operation error registry. Contains an entry for every modeled
                /// error this operation can throw.
                pub(crate) mod error_registry {
                    ##[allow(dead_code)]
                    pub(crate) static REGISTRY: #{LazyLock}<#{TypeRegistry}> = #{LazyLock}::new(|| {
                        #{TypeRegistry}::builder()
                            #{Entries}
                            .build()
                    });
                }
                """,
                "LazyLock" to lazyLock,
                "TypeRegistry" to typeRegistry,
                "Entries" to
                    writable {
                        errors.forEach { shape ->
                            val type = symbolProvider.toSymbol(shape)
                            rustTemplate(
                                """
                                .insert_shape(
                                    #{Type}::SCHEMA,
                                    |d: &mut dyn #{ShapeDeserializer}| -> #{Result}<#{TypeErasedBox}, #{SerdeError}> {
                                        #{Result}::Ok(#{TypeErasedBox}::new(#{Type}::deserialize(d)?))
                                    },
                                )
                                """,
                                "Type" to type,
                                "ShapeDeserializer" to shapeDeserializer,
                                "TypeErasedBox" to typeErasedBox,
                                "SerdeError" to serdeError,
                                "Result" to RuntimeType.std.resolve("result::Result"),
                            )
                        }
                    },
            )
        }
}
