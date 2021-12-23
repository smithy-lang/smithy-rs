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
import software.amazon.smithy.rust.codegen.util.orNull
import java.util.Optional
import java.util.logging.Logger
import kotlin.streams.toList

private const val SERVICE = "service"
private const val MODULE_NAME = "module"
private const val MODULE_DESCRIPTION = "moduleDescription"
private const val MODULE_VERSION = "moduleVersion"
private const val MODULE_AUTHORS = "moduleAuthors"
private const val MODULE_REPOSITORY = "moduleRepository"
private const val RUNTIME_CONFIG = "runtimeConfig"
private const val LICENSE = "license"
private const val EXAMPLES = "examples"
const val CODEGEN_SETTINGS = "codegen"

/**
 * Configuration of codegen settings
 *
 * [renameExceptions]: Rename `Exception` to `Error` in the generated SDK
 * [includeFluentClient]: Generate a `client` module in the generated SDK (currently the AWS SDK sets this to false
 *   and generates its own client)
 *
 * [addMessageToErrors]: Adds a `message` field automatically to all error shapes
 * [formatTimeoutSeconds]: Timeout for running cargo fmt at the end of code generation
 */
data class CodegenConfig(
    val renameExceptions: Boolean = true,
    val includeFluentClient: Boolean = true,
    val addMessageToErrors: Boolean = true,
    val formatTimeoutSeconds: Int = 20,
    // TODO(EventStream): [CLEANUP] Remove this property when turning on Event Stream for all services
    val eventStreamAllowList: Set<String> = emptySet(),
) {
    companion object {
        fun fromNode(node: Optional<ObjectNode>): CodegenConfig {
            return if (node.isPresent) {
                CodegenConfig(
                    node.get().getBooleanMemberOrDefault("renameErrors", true),
                    node.get().getBooleanMemberOrDefault("includeFluentClient", true),
                    node.get().getBooleanMemberOrDefault("addMessageToErrors", true),
                    node.get().getNumberMemberOrDefault("formatTimeoutSeconds", 20).toInt(),
                    node.get().getArrayMember("eventStreamAllowList")
                        .map { array -> array.toList().mapNotNull { node -> node.asStringNode().orNull()?.value } }
                        .orNull()?.toSet() ?: emptySet()
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
    val moduleDescription: String?,
    val moduleRepository: String?,
    val runtimeConfig: RuntimeConfig,
    val codegenConfig: CodegenConfig,
    val license: String?,
    val examplesUri: String? = null,
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
            val codegenSettings = config.getObjectMember(CODEGEN_SETTINGS)
            val codegenConfig = CodegenConfig.fromNode(codegenSettings)
            return fromCodegenConfig(model, config, codegenConfig)
        }

        /**
         * Create settings from a configuration object node and CodegenConfig.
         *
         * @param model Model to infer the service from (if not explicitly set in config)
         * @param config Config object to load
         * @param codegenConfig CodegenConfig object to use
         * @throws software.amazon.smithy.model.node.ExpectationNotMetException
         * @return Returns the extracted settings
         */
        fun fromCodegenConfig(model: Model, config: ObjectNode, codegenConfig: CodegenConfig): RustSettings {
            config.warnIfAdditionalProperties(
                arrayListOf(
                    SERVICE,
                    MODULE_NAME,
                    MODULE_DESCRIPTION,
                    MODULE_AUTHORS,
                    MODULE_VERSION,
                    MODULE_REPOSITORY,
                    RUNTIME_CONFIG,
                    CODEGEN_SETTINGS,
                    EXAMPLES,
                    LICENSE
                )
            )

            val service = config.getStringMember(SERVICE)
                .map(StringNode::expectShapeId)
                .orElseGet { inferService(model) }

            val runtimeConfig = config.getObjectMember(RUNTIME_CONFIG)
            return RustSettings(
                service = service,
                moduleName = config.expectStringMember(MODULE_NAME).value,
                moduleVersion = config.expectStringMember(MODULE_VERSION).value,
                moduleAuthors = config.expectArrayMember(MODULE_AUTHORS).map { it.expectStringNode().value },
                moduleDescription = config.getStringMember(MODULE_DESCRIPTION).orNull()?.value,
                moduleRepository = config.getStringMember(MODULE_REPOSITORY).orNull()?.value,
                runtimeConfig = RuntimeConfig.fromNode(runtimeConfig),
                codegenConfig,
                license = config.getStringMember(LICENSE).orNull()?.value,
                examplesUri = config.getStringMember(EXAMPLES).orNull()?.value,
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
