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
 *   the service closure. It is exposed publicly via `Client::error_registry()` for
 *   third-party `Document`-based reification — turning a discriminated `Document` into
 *   a typed error variant via [`TypeRegistry::deserialize_document`]. It also serves as
 *   the widening fallback for the per-operation scoped reification in the schema-serde
 *   error path.
 * - The per-operation error registry contains only the errors that operation can throw,
 *   and lives inside the operation's module as a `pub(crate)` static. The schema-serde
 *   error path consults it first (then widens to the service-wide registry) to reify an
 *   error code that does not match one of the operation's modeled errors directly.
 *
 * Only fires for services in [SchemaSerdeAllowlist]; non-allowlisted services pay no
 * artifact-size cost. Per-operation registries are only emitted for operations with
 * at least one modeled error to avoid empty registries.
 *
 * Future optimization: the per-operation registries exist solely to give the
 * scoped reification fallback operation-level precedence (an operation's own errors
 * win over a service-wide error sharing the same wire code). If that precedence is
 * later judged unnecessary, the per-operation registries can be dropped entirely and
 * the fallback can reify against the service-wide registry alone. Eliminating them
 * would reduce generated code, compile time, and binary size, at the cost of losing
 * operation-scoped precedence on wire-code collisions.
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
        val rc = codegenContext.runtimeConfig

        val smithySchema = RuntimeType.smithySchema(rc)
        val smithyTypes = RuntimeType.smithyTypes(rc)
        val typeRegistry = smithySchema.resolve("registry::TypeRegistry")
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
                "Entries" to registryEntries(codegenContext, errorShapes, emitErrorConstructor = true),
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
                    /// in the service closure, keyed by [`ShapeId`](#{ShapeId}). Use it to reify a
                    /// discriminated [`Document`](#{Document}) into a typed error variant via
                    /// [`TypeRegistry::deserialize_document`](#{TypeRegistry}::deserialize_document).
                    ///
                    /// This registry is provided for third-party `Document`-based error handling.
                    /// Internally it also serves as the widening fallback when the schema-serde
                    /// error path reifies an error code that an operation does not model directly
                    /// (the operation's own error registry is consulted first).
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
            val smithySchema = RuntimeType.smithySchema(rc)
            val typeRegistry = smithySchema.resolve("registry::TypeRegistry")
            val lazyLock = RuntimeType.std.resolve("sync::LazyLock")

            rustTemplate(
                """
                /// Per-operation error registry. Contains an entry for every modeled
                /// error this operation can throw.
                ///
                /// Used by the schema-serde error path as the operation-scoped lookup
                /// for registry-backed error reification: an error code that does not
                /// match one of the operation's modeled errors directly is resolved
                /// against this registry first, then widened to the service-wide error
                /// registry.
                pub(crate) mod error_registry {
                    pub(crate) static REGISTRY: #{LazyLock}<#{TypeRegistry}> = #{LazyLock}::new(|| {
                        #{TypeRegistry}::builder()
                            #{Entries}
                            .build()
                    });
                }
                """,
                "LazyLock" to lazyLock,
                "TypeRegistry" to typeRegistry,
                "Entries" to registryEntries(codegenContext, errors, emitErrorConstructor = true),
            )
        }
}
