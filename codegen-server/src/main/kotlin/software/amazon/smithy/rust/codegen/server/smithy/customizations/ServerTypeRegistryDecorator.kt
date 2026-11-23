/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerSchemaDeserializeGenerator
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput

/**
 * Generates `crate::type_registry::registry()`, a [TypeRegistry] with an entry for every structure in the
 * service closure, when `schemaSerde` is enabled. This is the server counterpart of the client's
 * `Client::registry()`: it lets code that only knows a shape ID at runtime, such as a document or an
 * envelope payload, find the shape's schema and read it with the generated schema walker.
 *
 * Each entry's deserializer runs the shape's `deserialize_schema` walker. A shape reachable from an operation
 * input comes back as its validating builder, so the entry calls `build()` and reports a constraint violation
 * as a [SerdeError]; every other shape comes back built. Structures with streaming members have no document
 * form and are not registered, and the synthetic operation input and output shapes are skipped like on the
 * client.
 *
 * [TypeRegistry]: aws_smithy_schema::registry::TypeRegistry
 * [SerdeError]: aws_smithy_schema::serde::SerdeError
 */
class ServerTypeRegistryDecorator : ServerCodegenDecorator {
    override val name: String = "ServerTypeRegistry"
    override val order: Byte = 0

    override fun extras(
        codegenContext: ServerCodegenContext,
        rustCrate: RustCrate,
    ) {
        if (!codegenContext.settings.codegenConfig.schemaSerde) {
            return
        }
        val model = codegenContext.model
        val smithySchema = RuntimeType.smithySchema(codegenContext.runtimeConfig)
        val shapes =
            DirectedWalker(model).walkShapes(codegenContext.serviceShape)
                .filterIsInstance<StructureShape>()
                .filter { !it.hasTrait<SyntheticInputTrait>() && !it.hasTrait<SyntheticOutputTrait>() }
                .filter { shape ->
                    shape.members().none { member ->
                        member.isEventStream(model) || model.expectShape(member.target).hasTrait<StreamingTrait>()
                    }
                }
                .sortedBy { it.id.toString() }

        rustCrate.withModule(typeRegistryModule) {
            rustTemplate(
                """
                static REGISTRY: #{LazyLock}<#{TypeRegistry}> = #{LazyLock}::new(|| {
                    #{TypeRegistry}::builder()
                        #{Entries:W}
                        .build()
                });

                /// Returns the type registry for this service.
                ///
                /// The registry has an entry for every structure in the service closure, except the
                /// operation input and output wrappers and structures with streaming members, keyed by
                /// shape ID. Each entry carries the shape's schema and a deserializer that reads the shape
                /// from a [`ShapeDeserializer`](#{ShapeDeserializer}) and validates its constraints.
                pub fn registry() -> &'static #{TypeRegistry} {
                    &REGISTRY
                }
                """,
                "LazyLock" to RuntimeType.std.resolve("sync::LazyLock"),
                "TypeRegistry" to smithySchema.resolve("registry::TypeRegistry"),
                "ShapeDeserializer" to smithySchema.resolve("serde::ShapeDeserializer"),
                "Entries" to entries(codegenContext, shapes),
            )
        }
    }

    private fun entries(
        codegenContext: ServerCodegenContext,
        shapes: List<StructureShape>,
    ): Writable =
        writable {
            val model = codegenContext.model
            val symbolProvider = codegenContext.symbolProvider
            val smithySchema = RuntimeType.smithySchema(codegenContext.runtimeConfig)
            val serdeError = smithySchema.resolve("serde::SerdeError")
            for (shape in shapes) {
                // The walker of an input shape with constraints returns the unconstrained builder.
                val returnsBuilder =
                    shape.isReachableFromOperationInput() && shape.canReachConstrainedShape(model, symbolProvider)
                rustTemplate(
                    """
                    .insert_shape(
                        #{Type}::SCHEMA,
                        |d: &mut dyn #{ShapeDeserializer}| -> #{Result}<#{TypeErasedBox}, #{SerdeError}> {
                            let value = #{Type}::${ServerSchemaDeserializeGenerator.FUNCTION_NAME}(d)?;
                            #{Finish:W}
                            #{Result}::Ok(#{TypeErasedBox}::new(value))
                        },
                    )
                    """,
                    *preludeScope,
                    "Type" to symbolProvider.toSymbol(shape),
                    "ShapeDeserializer" to smithySchema.resolve("serde::ShapeDeserializer"),
                    "SerdeError" to serdeError,
                    "TypeErasedBox" to RuntimeType.smithyTypes(codegenContext.runtimeConfig).resolve("type_erasure::TypeErasedBox"),
                    "Finish" to
                        writable {
                            if (returnsBuilder) {
                                rustTemplate(
                                    "let value = value.build().map_err(|violation| #{SerdeError}::custom(violation.to_string()))?;",
                                    "SerdeError" to serdeError,
                                )
                            }
                        },
                )
            }
        }

    companion object {
        val typeRegistryModule: RustModule.LeafModule =
            RustModule.public(
                "type_registry",
                documentationOverride = "Runtime lookup of this service's shapes by shape ID.",
            )
    }
}
