/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
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
import java.util.*
import java.util.logging.Logger
import kotlin.streams.toList

const val SERVICE = "service"
const val MODULE_NAME = "module"
const val MODULE_DESCRIPTION = "moduleDescription"
const val MODULE_VERSION = "moduleVersion"
const val MODULE_AUTHORS = "moduleAuthors"
const val MODULE_REPOSITORY = "moduleRepository"
const val RUNTIME_CONFIG = "runtimeConfig"
const val LICENSE = "license"
const val EXAMPLES = "examples"
const val CUSTOMIZATION_CONFIG = "customizationConfig"
const val CODEGEN_SETTINGS = "codegen"

/**
 * Configuration of codegen settings.
 *
 * [renameExceptions]: Rename `Exception` to `Error` in the generated SDK
 * [formatTimeoutSeconds]: Timeout for running cargo fmt at the end of code generation
 * [debugMode]: Generate comments in the generated code indicating where code was generated from
 */
// TODO Remove the default params, they should only be needed by TestSymbolVisitorDefaultConfig
open class CodegenConfig(
    open val formatTimeoutSeconds: Int = 20,
    open val debugMode: Boolean = false,
    // TODO(EventStream): [CLEANUP] Remove this property when turning on Event Stream for all services
    open val eventStreamAllowList: Set<String> = emptySet(),
)

/**
 * Settings used by [RustCodegenPlugin]
 */
open class RustSettings(
    open val service: ShapeId,
    open val moduleName: String,
    open val moduleVersion: String,
    open val moduleAuthors: List<String>,
    open val moduleDescription: String?,
    open val moduleRepository: String?,

    /**
     * Configuration of the runtime package:
     * - Where are the runtime crates (smithy-*) located on the file system? Or are they versioned?
     * - What are they called?
     */
    open val runtimeConfig: RuntimeConfig,
    open val codegenConfig: CodegenConfig,
    open val license: String?,
    open val examplesUri: String? = null,
    open val customizationConfig: ObjectNode? = null
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

        // Infer the service to generate from a model.
        @JvmStatic
        protected fun inferService(model: Model): ShapeId {
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

data class ClientRustSettings(
    override val service: ShapeId,
    override val moduleName: String,
    override val moduleVersion: String,
    override val moduleAuthors: List<String>,
    override val moduleDescription: String?,
    override val moduleRepository: String?,
    override val runtimeConfig: RuntimeConfig,
    override val codegenConfig: ClientCodegenConfig,
    override val license: String?,
    override val examplesUri: String? = null,
    override val customizationConfig: ObjectNode? = null
): RustSettings(
    service, moduleName, moduleVersion, moduleAuthors, moduleDescription, moduleRepository, runtimeConfig, codegenConfig, license
) {

    companion object {
        /**
         * Create settings from a configuration object node.
         *
         * @param model Model to infer the service from (if not explicitly set in config)
         * @param config Config object to load
         * @return Returns the extracted settings
         */
        fun from(model: Model, config: ObjectNode): ClientRustSettings {
            val codegenSettings = config.getObjectMember(CODEGEN_SETTINGS)
            val codegenConfig = ClientCodegenConfig.fromNode(codegenSettings)
            return fromCodegenConfig(model, config, codegenConfig)
        }

        /**
         * Create settings from a configuration object node and CodegenConfig.
         *
         * @param model Model to infer the service from (if not explicitly set in config)
         * @param config Config object to load
         * @param codegenConfig CodegenConfig object to use
         * @return Returns the extracted settings
         */
        private fun fromCodegenConfig(model: Model, config: ObjectNode, codegenConfig: ClientCodegenConfig): ClientRustSettings {
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
                    LICENSE,
                    CUSTOMIZATION_CONFIG
                )
            )

            val service = config.getStringMember(SERVICE)
                .map(StringNode::expectShapeId)
                .orElseGet { inferService(model) }

            val runtimeConfig = config.getObjectMember(RUNTIME_CONFIG)
            return ClientRustSettings(
                service,
                moduleName = config.expectStringMember(MODULE_NAME).value,
                moduleVersion = config.expectStringMember(MODULE_VERSION).value,
                moduleAuthors = config.expectArrayMember(MODULE_AUTHORS).map { it.expectStringNode().value },
                moduleDescription = config.getStringMember(MODULE_DESCRIPTION).orNull()?.value,
                moduleRepository = config.getStringMember(MODULE_REPOSITORY).orNull()?.value,
                runtimeConfig = RuntimeConfig.fromNode(runtimeConfig),
                codegenConfig,
                license = config.getStringMember(LICENSE).orNull()?.value,
                examplesUri = config.getStringMember(EXAMPLES).orNull()?.value,
                customizationConfig = config.getObjectMember(CUSTOMIZATION_CONFIG).orNull()
            )
        }
    }
}

/**
 * [includeFluentClient]: Generate a `client` module in the generated SDK (currently the AWS SDK sets this to `false`
 *   and generates its own client)
 * [addMessageToErrors]: Adds a `message` field automatically to all error shapes
 */
data class ClientCodegenConfig(
    override val formatTimeoutSeconds: Int = 20,
    override val debugMode: Boolean = false,
    override val eventStreamAllowList: Set<String> = emptySet(),
    val renameExceptions: Boolean = true,
    val includeFluentClient: Boolean = true,
    val addMessageToErrors: Boolean = true,
): CodegenConfig(
    formatTimeoutSeconds, debugMode, eventStreamAllowList
) {
    companion object {
        fun fromNode(node: Optional<ObjectNode>): ClientCodegenConfig =
            if (node.isPresent) {
                ClientCodegenConfig(
                    node.get().getNumberMemberOrDefault("formatTimeoutSeconds", 20).toInt(),
                    node.get().getBooleanMemberOrDefault("debugMode", false),
                    node.get().getArrayMember("eventStreamAllowList")
                        .map { array -> array.toList().mapNotNull { node -> node.asStringNode().orNull()?.value } }
                        .orNull()?.toSet() ?: emptySet(),
                    node.get().getBooleanMemberOrDefault("includeFluentClient", true),
                    node.get().getBooleanMemberOrDefault("addMessageToErrors", true),
                    node.get().getBooleanMemberOrDefault("renameErrors", true),
                )
            } else {
                ClientCodegenConfig()
            }
    }
}
