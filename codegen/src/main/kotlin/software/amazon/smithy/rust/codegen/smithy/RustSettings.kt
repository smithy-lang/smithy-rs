/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.orNull
import java.util.Optional
import java.util.logging.Logger
import kotlin.streams.toList

private const val SERVICE = "service"
private const val MODULE_NAME = "module"
private const val MODULE_DESCRIPTION = "moduleDescription"
private const val MODULE_VERSION = "moduleVersion"
private const val MODULE_AUTHORS = "moduleAuthors"
private const val RUNTIME_CONFIG = "runtimeConfig"
private const val CODEGEN_SETTINGS = "codegen"
private const val LICENSE = "license"

data class CodegenConfig(val renameExceptions: Boolean = true, val includeFluentClient: Boolean = true, val formatTimeoutSeconds: Int) {
    companion object {
        fun fromNode(node: Optional<ObjectNode>): CodegenConfig {
            return if (node.isPresent) {
                CodegenConfig(
                    node.get().getBooleanMemberOrDefault("renameErrors", true),
                    node.get().getBooleanMemberOrDefault("includeFluentClient", true),
                    node.get().getNumberMemberOrDefault("formatTimeoutSeconds", 20).toInt()
                )
            } else {
                CodegenConfig()
            }
        }
    }
}

/**
 * Settings used by [RustCodegenPlugin]
 */
class RustSettings(
    val service: ShapeId,
    val moduleName: String,
    val moduleVersion: String,
    val moduleAuthors: List<String>,
    val runtimeConfig: RuntimeConfig,
    val codegenConfig: CodegenConfig,
    val license: String?,
    private val model: Model
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

    val moduleDescription: String
        get() = getService(model).getTrait<DocumentationTrait>()?.value ?: moduleName

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
            config.warnIfAdditionalProperties(
                arrayListOf(
                    SERVICE,
                    MODULE_NAME,
                    MODULE_DESCRIPTION,
                    MODULE_AUTHORS,
                    MODULE_VERSION,
                    RUNTIME_CONFIG,
                    CODEGEN_SETTINGS,
                    LICENSE
                )
            )

            val service = config.getStringMember(SERVICE)
                .map(StringNode::expectShapeId)
                .orElseGet { inferService(model) }

            val moduleName = config.expectStringMember(MODULE_NAME).value
            val version = config.expectStringMember(MODULE_VERSION).value
            val runtimeConfig = config.getObjectMember(RUNTIME_CONFIG)
            val codegenSettings = config.getObjectMember(CODEGEN_SETTINGS)
            val moduleAuthors = config.expectArrayMember(MODULE_AUTHORS).map { it.expectStringNode().value }
            val license = config.getStringMember(LICENSE).orNull()?.value
            return RustSettings(
                service = service,
                moduleName = moduleName,
                moduleVersion = version,
                moduleAuthors = moduleAuthors,
                runtimeConfig = RuntimeConfig.fromNode(runtimeConfig),
                codegenConfig = CodegenConfig.fromNode(codegenSettings),
                license = license,
                model = model
            )
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
}
