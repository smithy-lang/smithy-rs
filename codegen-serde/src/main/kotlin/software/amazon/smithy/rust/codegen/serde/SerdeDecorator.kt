/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.serde

import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator

val SerdeFeature = Feature("serde", false, listOf("dep:serde"))
val SerdeModule =
    RustModule.public(
        "serde",
        additionalAttributes = listOf(Attribute.featureGate(SerdeFeature.name)),
        documentationOverride = "Implementations of `serde` for model types. NOTE: These implementations are NOT used for wire serialization as part of a Smithy protocol and WILL NOT match the wire format. They are provided for convenience only.",
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
    val roots = serializationRoots(codegenContext)
    if (roots.isNotEmpty()) {
        rustCrate.mergeFeature(SerdeFeature)
        val generator = SerializeImplGenerator(codegenContext)
        rustCrate.withModule(SerdeModule) {
            roots.forEach {
                generator.generateRootSerializerForShape(it)(this)
            }
            addDependency(SupportStructures.serializeRedacted().toSymbol())
            addDependency(SupportStructures.serializeUnredacted().toSymbol())
        }
    }
}

/**
 * All entry points for serialization in the service closure.
 */
fun serializationRoots(ctx: CodegenContext): List<Shape> {
    val serviceShape = ctx.serviceShape
    val walker = Walker(ctx.model)
    return walker.walkShapes(serviceShape).filter { it.hasTrait<SerdeTrait>() }
}
