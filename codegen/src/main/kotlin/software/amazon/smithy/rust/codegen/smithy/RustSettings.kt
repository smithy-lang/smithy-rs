/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import java.util.Optional
import java.util.logging.Logger
import kotlin.streams.toList
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId

private const val SERVICE = "service"
private const val MODULE_NAME = "module"
private const val MODULE_DESCRIPTION = "moduleDescription"
private const val MODULE_VERSION = "moduleVersion"
private const val BUILD_SETTINGS = "build"
private const val RUNTIME_CONFIG = "runtimeConfig"

/**
 * Settings used by [RustCodegenPlugin]
 */
class RustSettings(
    val service: ShapeId,
    val moduleName: String,
    val moduleVersion: String,
    val moduleDescription: String = "",
    val runtimeConfig: RuntimeConfig,
    val build: BuildSettings
) {

    /**
     * Get the corresponding [ServiceShape] from a model.
     * @return Returns the found `Service`
     * @throws CodegenException if the service is invalid or not found
     */
    fun getService(model: Model): ServiceShape {
        return model
            .getShape(service)
            .orElseThrow { CodegenException("Service shape not found: $service") }
            .asServiceShape()
            .orElseThrow { CodegenException("Shape is not a service: $service") }
    }

    companion object {
        private val LOGGER: Logger = Logger.getLogger(RustSettings::class.java.name)

        /**
         * Create settings from a configuration object node.
         *
         * @param model Model to infer the service from (if not explicitly set in config)
         * @param config Config object to load
         * @throws software.amazon.smithy.model.node.ExpectationNotMetException
         * @return Returns the extracted settings
         */
        fun from(model: Model, config: ObjectNode): RustSettings {
            config.warnIfAdditionalProperties(arrayListOf(SERVICE, MODULE_NAME, MODULE_DESCRIPTION, MODULE_VERSION, BUILD_SETTINGS, RUNTIME_CONFIG))

            val service = config.getStringMember(SERVICE)
                .map(StringNode::expectShapeId)
                .orElseGet { inferService(model) }

            val moduleName = config.expectStringMember(MODULE_NAME).value
            val version = config.expectStringMember(MODULE_VERSION).value
            val desc = config.getStringMemberOrDefault(MODULE_DESCRIPTION, "$moduleName client")
            val build = config.getObjectMember(BUILD_SETTINGS)
            val runtimeConfig = config.getObjectMember(RUNTIME_CONFIG)
            println("run config: $runtimeConfig -- ${RuntimeConfig.fromNode(runtimeConfig)}")
            return RustSettings(service, moduleName, version, desc, RuntimeConfig.fromNode(runtimeConfig), BuildSettings.fromNode(build))
        }

        // infer the service to generate from a model
        private fun inferService(model: Model): ShapeId {
            val services = model.shapes(ServiceShape::class.java)
                .map(Shape::getId)
                .sorted()
                .toList()

            when {
                services.isEmpty() -> {
                    throw CodegenException(
                        "Cannot infer a service to generate because the model does not " +
                                "contain any service shapes"
                    )
                }
                services.size > 1 -> {
                    throw CodegenException(
                        "Cannot infer service to generate because the model contains " +
                                "multiple service shapes: " + services
                    )
                }
                else -> {
                    val service = services[0]
                    LOGGER.info("Inferring service to generate as: $service")
                    return service
                }
            }
        }
    }

    /**
     * Resolves the highest priority protocol from a service shape that is
     * supported by the generator.
     *
     * @param serviceIndex Service index containing the support
     * @param service Service to get the protocols from if "protocols" is not set.
     * @param supportedProtocolTraits The set of protocol traits supported by the generator.
     * @return Returns the resolved protocol name.
     * @throws UnresolvableProtocolException if no protocol could be resolved.
     */
    fun resolveServiceProtocol(
        serviceIndex: ServiceIndex,
        service: ServiceShape,
        supportedProtocolTraits: Set<ShapeId>
    ): ShapeId {
        val resolvedProtocols: Set<ShapeId> = serviceIndex.getProtocols(service).keys
        val protocol = resolvedProtocols.firstOrNull(supportedProtocolTraits::contains)
        return protocol ?: throw UnresolvableProtocolException(
            "The ${service.id} service supports the following unsupported protocols $resolvedProtocols. " +
                    "The following protocol generators were found on the class path: $supportedProtocolTraits")
    }
}

data class BuildSettings(val rootProject: Boolean = false) {
    companion object {
        private const val ROOT_PROJECT = "rootProject"
        fun fromNode(node: Optional<ObjectNode>): BuildSettings {
            return if (node.isPresent) {
                BuildSettings(node.get().getMember(BuildSettings.ROOT_PROJECT).get().asBooleanNode().get().value)
            } else {
                Default()
            }
        }
        fun Default(): BuildSettings = BuildSettings(false)
    }
}

class UnresolvableProtocolException(message: String) : CodegenException(message)
