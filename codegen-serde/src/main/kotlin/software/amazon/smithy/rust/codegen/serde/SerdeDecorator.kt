/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.serde

import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator

val SerdeFeature = Feature("serde", false, listOf("dep:serde"))
val SerdeModule =
    RustModule.public(
        "serde",
        additionalAttributes = listOf(Attribute.featureGate(SerdeFeature.name)),
        documentationOverride = "Configurable `serde` serialization and deserialization support for model types. These conversions are provided for convenience only. They are not used for Smithy protocol wire serialization or deserialization and are not guaranteed to match any protocol wire format.",
    )

class ClientSerdeDecorator : ClientCodegenDecorator {
    override val name: String = "ClientSerdeDecorator"
    override val order: Byte = 0

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) = extrasCommon(codegenContext, rustCrate)
}

class ServerSerdeDecorator : ServerCodegenDecorator {
    override val name: String = "ServerSerdeDecorator"
    override val order: Byte = 0

    override fun extras(
        codegenContext: ServerCodegenContext,
        rustCrate: RustCrate,
    ) = extrasCommon(codegenContext, rustCrate)
}

// Just a common function to keep things DRY.
private fun extrasCommon(
    codegenContext: CodegenContext,
    rustCrate: RustCrate,
) {
    val serializationRoots = serializationRoots(codegenContext)
    val deserializationRoots = deserializationRoots(codegenContext)
    if (serializationRoots.isNotEmpty() || deserializationRoots.isNotEmpty()) {
        rustCrate.mergeFeature(SerdeFeature)
        rustCrate.withModule(SerdeModule) {
            if (serializationRoots.isNotEmpty()) {
                val generator = SerializeImplGenerator(codegenContext)
                serializationRoots.forEach {
                    generator.generateRootSerializerForShape(it)(this)
                }
                addDependency(SupportStructures.serializeRedacted().toSymbol())
                addDependency(SupportStructures.serializeUnredacted().toSymbol())
            }
            if (deserializationRoots.isNotEmpty()) {
                val generator = DeserializeImplGenerator(codegenContext)
                deserializationRoots.forEach {
                    generator.generateRootDeserializerForShape(it)(this)
                }
                addDependency(SupportStructures.serdeDependency().toSymbol())
            }
        }
    }
}

/**
 * All entry points for serialization in the service closure.
 */
fun serializationRoots(ctx: CodegenContext): List<Shape> {
    return serdeRoots(ctx) { it.serialize }
}

/**
 * All nominal entry points for deserialization in the service closure.
 */
fun deserializationRoots(ctx: CodegenContext): List<Shape> =
    serdeRoots(ctx) { it.deserialize }
        .filter {
            it is StructureShape ||
                it is UnionShape ||
                it is EnumShape ||
                (it is StringShape && it.hasTrait<EnumTrait>()) ||
                it is OperationShape ||
                it is ServiceShape
        }

private fun serdeRoots(
    ctx: CodegenContext,
    directionEnabled: (SerdeTrait) -> Boolean,
): List<Shape> =
    Walker(ctx.model)
        .walkShapes(ctx.serviceShape)
        .filter { shape -> shape.getTrait<SerdeTrait>()?.let(directionEnabled) == true }
