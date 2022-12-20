/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.util.orNull
import java.util.Optional
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
 * [CoreCodegenConfig] contains code-generation configuration that is _common to all_  smithy-rs plugins.
 *
 * If your configuration is specific to the `rust-client-codegen` client plugin, put it in [ClientCodegenContext] instead.
 * If your configuration is specific to the `rust-server-codegen` server plugin, put it in [ServerCodegenContext] instead.
 *
 * [formatTimeoutSeconds]: Timeout for running cargo fmt at the end of code generation
 * [debugMode]: Generate comments in the generated code indicating where code was generated from
 */
open class CoreCodegenConfig(
    open val formatTimeoutSeconds: Int = defaultFormatTimeoutSeconds,
    open val debugMode: Boolean = defaultDebugMode,
) {
    companion object {
        const val defaultFormatTimeoutSeconds = 20
        const val defaultDebugMode = false

        fun fromNode(node: Optional<ObjectNode>): CoreCodegenConfig =
            if (node.isPresent) {
                CoreCodegenConfig(
                    node.get().getNumberMemberOrDefault("formatTimeoutSeconds", defaultFormatTimeoutSeconds).toInt(),
                    node.get().getBooleanMemberOrDefault("debugMode", defaultDebugMode),
                )
            } else {
                CoreCodegenConfig(
                    formatTimeoutSeconds = defaultFormatTimeoutSeconds,
                    debugMode = defaultDebugMode,
                )
            }
    }
}

/**
 * [CoreRustSettings] contains crate settings that are _common to all_  smithy-rs plugins.
 *
 * If your setting is specific to the crate that the `rust-client-codegen` client plugin generates, put it in
 * [ClientCodegenContext] instead.
 * If your setting is specific to the crate that the `rust-server-codegen` server plugin generates, put it in
 * [ServerCodegenContext] instead.
 */
open class CoreRustSettings(
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
    open val codegenConfig: CoreCodegenConfig,
    open val license: String?,
    open val examplesUri: String? = null,
    open val customizationConfig: ObjectNode? = null,
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
        private val LOGGER: Logger = Logger.getLogger(CoreRustSettings::class.java.name)

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
                            "contain any service shapes",
                    )
                }
                services.size > 1 -> {
                    throw CodegenException(
                        "Cannot infer service to generate because the model contains " +
                            "multiple service shapes: " + services,
                    )
                }
                else -> {
                    val service = services[0]
                    LOGGER.info("Inferring service to generate as: $service")
                    return service
                }
            }
        }

        /**
         * Create settings from a configuration object node.
         *
         * @param model Model to infer the service from (if not explicitly set in config)
         * @param config Config object to load
         * @return Returns the extracted settings
         */
        fun from(model: Model, config: ObjectNode): CoreRustSettings {
            val codegenSettings = config.getObjectMember(CODEGEN_SETTINGS)
            val coreCodegenConfig = CoreCodegenConfig.fromNode(codegenSettings)
            return fromCodegenConfig(model, config, coreCodegenConfig)
        }

        /**
         * Create settings from a configuration object node and CodegenConfig.
         *
         * @param model Model to infer the service from (if not explicitly set in config)
         * @param config Config object to load
         * @param coreCodegenConfig CodegenConfig object to use
         * @return Returns the extracted settings
         */
        private fun fromCodegenConfig(model: Model, config: ObjectNode, coreCodegenConfig: CoreCodegenConfig): CoreRustSettings {
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
                    CUSTOMIZATION_CONFIG,
                ),
            )

            val service = config.getStringMember(SERVICE)
                .map(StringNode::expectShapeId)
                .orElseGet { inferService(model) }

            val runtimeConfig = config.getObjectMember(RUNTIME_CONFIG)
            return CoreRustSettings(
                service,
                moduleName = config.expectStringMember(MODULE_NAME).value,
                moduleVersion = config.expectStringMember(MODULE_VERSION).value,
                moduleAuthors = config.expectArrayMember(MODULE_AUTHORS).map { it.expectStringNode().value },
                moduleDescription = config.getStringMember(MODULE_DESCRIPTION).orNull()?.value,
                moduleRepository = config.getStringMember(MODULE_REPOSITORY).orNull()?.value,
                runtimeConfig = RuntimeConfig.fromNode(runtimeConfig),
                codegenConfig = coreCodegenConfig,
                license = config.getStringMember(LICENSE).orNull()?.value,
                examplesUri = config.getStringMember(EXAMPLES).orNull()?.value,
                customizationConfig = config.getObjectMember(CUSTOMIZATION_CONFIG).orNull(),
            )
        }
    }
}
