/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait

/**
 * Generates a `Client::registry()` accessor returning a `&'static TypeRegistry` containing
 * all user-modeled structure shapes in the service's closure.
 *
 * Per the SEP "Document Type and Type Registries", the primary type registry includes
 * every user-modeled structure shape in the service closure:
 * - `@error` structures are included — the SEP requires the primary registry to hold
 *   every structure shape, and a shape may live in both the primary registry and the
 *   dedicated `Client::error_registry()`.
 * - Unions are excluded per SEP §"What to include in the package level Type Registries".
 * - Synthetic operation input/output shapes are excluded — they're codegen artifacts,
 *   not user-modeled types, and would inflate the registry with synthetic shape IDs
 *   that third-party callers would never look up.
 *
 * Only fires for services in [SchemaSerdeAllowlist]; non-allowlisted services pay no
 * artifact-size cost for this codegen.
 */
class TypeRegistryDecorator : ClientCodegenDecorator {
    override val name: String = "TypeRegistry"
    override val order: Byte = 0

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

        // Walk the service closure for structures, filtering out only synthetic
        // operation input/output shapes (codegen artifacts). `@error` structures are
        // intentionally kept: the SEP requires the primary registry to include every
        // structure shape, and an error may additionally appear in error_registry().
        // Sort by shape ID for deterministic output.
        val shapes =
            DirectedWalker(model).walkShapes(codegenContext.serviceShape)
                .filterIsInstance<StructureShape>()
                .filter { !it.hasTrait(SyntheticInputTrait::class.java) }
                .filter { !it.hasTrait(SyntheticOutputTrait::class.java) }
                .sortedBy { it.id.toString() }

        // Emit `crate::type_registry` private module with the LazyLock<TypeRegistry> static.
        val registryModule = RustModule.private("type_registry")
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
                "Entries" to registryEntries(codegenContext, shapes),
            )
        }

        // Add `impl Client { pub fn registry() -> &'static TypeRegistry { ... } }`
        // alongside the Client struct definition in `crate::client`.
        rustCrate.withModule(ClientRustModule.client) {
            rustTemplate(
                """
                impl Client {
                    /// Returns the type registry for this service.
                    ///
                    /// The registry contains an entry for every structure shape in the service
                    /// closure (excluding only synthetic operation input/output shapes),
                    /// keyed by [`ShapeId`](#{ShapeId}). It is primarily useful for deserializing
                    /// a [`Document`](#{Document}) into a typed shape:
                    ///
                    /// ```ignore
                    /// let registry = MyClient::registry();
                    /// let typed = registry.deserialize_document(&doc)?;
                    /// // Downcast to the concrete type you expected.
                    /// ```
                    pub fn registry() -> &'static #{TypeRegistry} {
                        &crate::type_registry::REGISTRY
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
