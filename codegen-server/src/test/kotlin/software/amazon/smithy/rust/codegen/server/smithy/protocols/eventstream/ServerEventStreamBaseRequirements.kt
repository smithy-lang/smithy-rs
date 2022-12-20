/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.eventstream

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestRequirements
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenConfig
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderGeneratorWithoutPublicConstrainedTypes
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestRustSettings

data class TestCase(
    val eventStreamTestCase: EventStreamTestModels.TestCase,
    val publicConstrainedTypes: Boolean,
) {
    override fun toString(): String = "$eventStreamTestCase, publicConstrainedTypes = $publicConstrainedTypes"
}

abstract class ServerEventStreamBaseRequirements : EventStreamTestRequirements<ServerCodegenContext> {
    abstract val publicConstrainedTypes: Boolean

    override fun createCodegenContext(
        model: Model,
        serviceShape: ServiceShape,
        protocolShapeId: ShapeId,
        codegenTarget: CodegenTarget,
    ): ServerCodegenContext = serverTestCodegenContext(
        model, serviceShape,
        serverTestRustSettings(
            codegenConfig = ServerCodegenConfig(publicConstrainedTypes = publicConstrainedTypes),
        ),
        protocolShapeId,
    )

    override fun renderBuilderForShape(
        writer: RustWriter,
        codegenContext: ServerCodegenContext,
        shape: StructureShape,
    ) {
        if (codegenContext.settings.codegenConfig.publicConstrainedTypes) {
            ServerBuilderGenerator(codegenContext, shape).apply {
                render(writer)
                writer.implBlock(shape, codegenContext.symbolProvider) {
                    renderConvenienceMethod(writer)
                }
            }
        } else {
            ServerBuilderGeneratorWithoutPublicConstrainedTypes(codegenContext, shape).apply {
                render(writer)
                writer.implBlock(shape, codegenContext.symbolProvider) {
                    renderConvenienceMethod(writer)
                }
            }
        }
    }
}
