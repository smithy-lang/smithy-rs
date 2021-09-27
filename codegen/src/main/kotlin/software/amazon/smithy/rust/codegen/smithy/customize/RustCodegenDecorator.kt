/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.customize

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.RustSettings
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.FluentClientDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolMap
import java.util.ServiceLoader
import java.util.logging.Logger

/**
 * [RustCodegenDecorator] allows downstream users to customize code generation.
 *
 * For example, AWS-specific code generation generates customizations required to support
 * AWS services. A different downstream customer way wish to add a different set of derive
 * attributes to the generated classes.
 */
interface RustCodegenDecorator {
    /**
     * The name of this [RustCodegenDecorator], used for logging and debug information
     */
    val name: String

    /**
     * Enable a deterministic ordering to be applied, with the lowest numbered integrations being applied first
     */
    val order: Byte

    fun configCustomizations(
        rustSettings: RustSettings,
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> = baseCustomizations

    fun operationCustomizations(
        rustSettings: RustSettings,
        protocolConfig: ProtocolConfig,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> = baseCustomizations

    fun libRsCustomizations(
        rustSettings: RustSettings,
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> = baseCustomizations

    fun extras(rustSettings: RustSettings, protocolConfig: ProtocolConfig, rustCrate: RustCrate) {}

    fun protocols(rustSettings: RustSettings, serviceId: ShapeId, currentProtocols: ProtocolMap): ProtocolMap =
        currentProtocols

    fun transformModel(rustSettings: RustSettings, service: ServiceShape, model: Model): Model = model

    fun symbolProvider(rustSettings: RustSettings, baseProvider: RustSymbolProvider): RustSymbolProvider = baseProvider
}

/**
 * [CombinedCodegenDecorator] merges the results of multiple decorators into a single decorator.
 *
 * This makes the actual concrete codegen simpler by not needing to deal with multiple separate decorators.
 */
open class CombinedCodegenDecorator(decorators: List<RustCodegenDecorator>) : RustCodegenDecorator {
    private val orderedDecorators = decorators.sortedBy { it.order }
    override val name: String
        get() = "MetaDecorator"
    override val order: Byte
        get() = 0

    override fun configCustomizations(
        rustSettings: RustSettings,
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return orderedDecorators.foldRight(baseCustomizations) { decorator: RustCodegenDecorator, customizations ->
            decorator.configCustomizations(rustSettings, protocolConfig, customizations)
        }
    }

    override fun operationCustomizations(
        rustSettings: RustSettings,
        protocolConfig: ProtocolConfig,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        return orderedDecorators.foldRight(baseCustomizations) { decorator: RustCodegenDecorator, customizations ->
            decorator.operationCustomizations(rustSettings, protocolConfig, operation, customizations)
        }
    }

    override fun libRsCustomizations(
        rustSettings: RustSettings,
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        return orderedDecorators.foldRight(baseCustomizations) { decorator, customizations ->
            decorator.libRsCustomizations(
                rustSettings,
                protocolConfig,
                customizations
            )
        }
    }

    override fun protocols(rustSettings: RustSettings, serviceId: ShapeId, currentProtocols: ProtocolMap): ProtocolMap {
        return orderedDecorators.foldRight(currentProtocols) { decorator, protocolMap ->
            decorator.protocols(rustSettings, serviceId, protocolMap)
        }
    }

    override fun symbolProvider(rustSettings: RustSettings, baseProvider: RustSymbolProvider): RustSymbolProvider {
        return orderedDecorators.foldRight(baseProvider) { decorator, provider ->
            decorator.symbolProvider(
                rustSettings,
                provider
            )
        }
    }

    override fun extras(rustSettings: RustSettings, protocolConfig: ProtocolConfig, rustCrate: RustCrate) {
        return orderedDecorators.forEach { it.extras(rustSettings, protocolConfig, rustCrate) }
    }

    override fun transformModel(rustSettings: RustSettings, service: ServiceShape, baseModel: Model): Model {
        return orderedDecorators.foldRight(baseModel) { decorator, model ->
            decorator.transformModel(
                rustSettings,
                service,
                model
            )
        }
    }

    companion object {
        private val logger = Logger.getLogger("RustCodegenSPILoader")
        fun fromClasspath(context: PluginContext): RustCodegenDecorator {
            val decorators = ServiceLoader.load(
                RustCodegenDecorator::class.java,
                context.pluginClassLoader.orElse(RustCodegenDecorator::class.java.classLoader)
            )
                .onEach {
                    logger.info("Adding Codegen Decorator: ${it.javaClass.name}")
                }.toList()
            return CombinedCodegenDecorator(decorators + RequiredCustomizations() + FluentClientDecorator())
        }
    }
}
