/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customize

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.smithy.customize.CombinedCoreCodegenDecorator
import software.amazon.smithy.rust.codegen.core.smithy.customize.CoreCodegenDecorator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolMap
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolGenerator
import java.util.logging.Logger

typealias ServerProtocolMap = ProtocolMap<ServerProtocolGenerator, ServerCodegenContext>

/**
 * [ServerCodegenDecorator] allows downstream users to customize code generation.
 */
interface ServerCodegenDecorator : CoreCodegenDecorator<ServerCodegenContext> {
    fun protocols(serviceId: ShapeId, currentProtocols: ServerProtocolMap): ServerProtocolMap = currentProtocols
}

/**
 * [CombinedServerCodegenDecorator] merges the results of multiple decorators into a single decorator.
 *
 * This makes the actual concrete codegen simpler by not needing to deal with multiple separate decorators.
 */
class CombinedServerCodegenDecorator(decorators: List<ServerCodegenDecorator>) :
    CombinedCoreCodegenDecorator<ServerCodegenContext, ServerCodegenDecorator>(decorators),
    ServerCodegenDecorator {
    override val name: String
        get() = "CombinedServerCodegenDecorator"
    override val order: Byte
        get() = 0

    override fun protocols(serviceId: ShapeId, currentProtocols: ServerProtocolMap): ServerProtocolMap =
        combineCustomizations(currentProtocols) { decorator, protocolMap ->
            decorator.protocols(serviceId, protocolMap)
        }

    companion object {
        fun fromClasspath(
            context: PluginContext,
            vararg extras: ServerCodegenDecorator,
            logger: Logger = Logger.getLogger("RustServerCodegenSPILoader"),
        ): CombinedServerCodegenDecorator {
            val decorators = decoratorsFromClasspath(context, ServerCodegenDecorator::class.java, logger, *extras)
            return CombinedServerCodegenDecorator(decorators)
        }
    }
}
