/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.smithy.CODEGEN_SETTINGS
import software.amazon.smithy.rust.codegen.core.smithy.CoreCodegenConfig
import software.amazon.smithy.rust.codegen.core.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.core.smithy.HttpVersion
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import java.util.Optional

/*
 * [ServerRustSettings] and [ServerCodegenConfig] classes.
 *
 * These classes are entirely analogous to [ClientRustSettings] and [ClientCodegenConfig]. Refer to the documentation
 * for those.
 *
 * These classes have to live in the `codegen` subproject because they are referenced in [ServerCodegenContext],
 * which is used in common generators to both client and server.
 */

/**
 * Settings used by [RustServerCodegenPlugin].
 */
data class ServerRustSettings(
    override val service: ShapeId,
    override val moduleName: String,
    override val moduleVersion: String,
    override val moduleAuthors: List<String>,
    override val moduleDescription: String?,
    override val moduleRepository: String?,
    override val runtimeConfig: RuntimeConfig,
    override val codegenConfig: ServerCodegenConfig,
    override val license: String?,
    override val examplesUri: String?,
    override val minimumSupportedRustVersion: String? = null,
    override val customizationConfig: ObjectNode?,
) : CoreRustSettings(
        service,
        moduleName,
        moduleVersion,
        moduleAuthors,
        moduleDescription,
        moduleRepository,
        runtimeConfig,
        codegenConfig,
        license,
        examplesUri,
        minimumSupportedRustVersion,
        customizationConfig,
    ) {
    companion object {
        fun from(
            model: Model,
            config: ObjectNode,
        ): ServerRustSettings {
            val coreRustSettings = CoreRustSettings.from(model, config)
            val codegenSettingsNode = config.getObjectMember(CODEGEN_SETTINGS)
            val coreCodegenConfig = CoreCodegenConfig.fromNode(codegenSettingsNode)

            // Create ServerCodegenConfig first to read the http-1x flag
            val serverCodegenConfig = ServerCodegenConfig.fromCodegenConfigAndNode(coreCodegenConfig, codegenSettingsNode)

            // Use the http1x field from ServerCodegenConfig to set RuntimeConfig httpVersion
            // This must be done because RuntimeConfig is created in CoreRustSettings.from()
            // before we have access to the http-1x flag
            val httpVersion = if (serverCodegenConfig.http1x) HttpVersion.Http1x else HttpVersion.Http0x
            val runtimeConfig = coreRustSettings.runtimeConfig.copy(httpVersion = httpVersion)

            return ServerRustSettings(
                service = coreRustSettings.service,
                moduleName = coreRustSettings.moduleName,
                moduleVersion = coreRustSettings.moduleVersion,
                moduleAuthors = coreRustSettings.moduleAuthors,
                moduleDescription = coreRustSettings.moduleDescription,
                moduleRepository = coreRustSettings.moduleRepository,
                runtimeConfig = runtimeConfig,
                codegenConfig = serverCodegenConfig,
                license = coreRustSettings.license,
                examplesUri = coreRustSettings.examplesUri,
                minimumSupportedRustVersion = coreRustSettings.minimumSupportedRustVersion,
                customizationConfig = coreRustSettings.customizationConfig,
            )
        }
    }
}

/**
 * [publicConstrainedTypes]: Generate constrained wrapper newtypes for constrained shapes
 * [ignoreUnsupportedConstraints]: Generate model even though unsupported constraints are present
 * [http1x]: Enable HTTP 1.x support (hyper 1.x and http 1.x types)
 */
data class ServerCodegenConfig(
    override val formatTimeoutSeconds: Int = DEFAULT_FORMAT_TIMEOUT_SECONDS,
    override val debugMode: Boolean = DEFAULT_DEBUG_MODE,
    val publicConstrainedTypes: Boolean = DEFAULT_PUBLIC_CONSTRAINED_TYPES,
    val ignoreUnsupportedConstraints: Boolean = DEFAULT_IGNORE_UNSUPPORTED_CONSTRAINTS,
    /**
     * A flag to enable _experimental_ support for custom validation exceptions via the
     * [CustomValidationExceptionWithReasonDecorator] decorator.
     * TODO(https://github.com/smithy-lang/smithy-rs/pull/2053): this will go away once we implement the RFC, when users will be
     *  able to define the converters in their Rust application code.
     */
    val experimentalCustomValidationExceptionWithReasonPleaseDoNotUse: String? = defaultExperimentalCustomValidationExceptionWithReasonPleaseDoNotUse,
    val addValidationExceptionToConstrainedOperations: Boolean = DEFAULT_ADD_VALIDATION_EXCEPTION_TO_CONSTRAINED_OPERATIONS,
    val alwaysSendEventStreamInitialResponse: Boolean = DEFAULT_SEND_EVENT_STREAM_INITIAL_RESPONSE,
    val http1x: Boolean = DEFAULT_HTTP_1X,
) : CoreCodegenConfig(
        formatTimeoutSeconds, debugMode,
    ) {
    companion object {
        private const val DEFAULT_PUBLIC_CONSTRAINED_TYPES = true
        private const val DEFAULT_IGNORE_UNSUPPORTED_CONSTRAINTS = false
        private val defaultExperimentalCustomValidationExceptionWithReasonPleaseDoNotUse = null
        private const val DEFAULT_ADD_VALIDATION_EXCEPTION_TO_CONSTRAINED_OPERATIONS = false
        private const val DEFAULT_SEND_EVENT_STREAM_INITIAL_RESPONSE = false
        const val DEFAULT_HTTP_1X = false

        /**
         * Configuration key for the HTTP 1.x flag.
         *
         * When set to true in codegen configuration, generates code that uses http@1.x/hyper@1.x
         * instead of http@0.2.x/hyper@0.14.x.
         *
         * **Usage:**
         * - Use this constant when reading/writing the codegen configuration
         * - Use this constant in test utilities that set configuration (e.g., ServerCodegenIntegrationTest)
         *
         * **Do NOT use this constant for:**
         * - External crate feature names (e.g., `smithyRuntimeApi.withFeature("http-1x")`)
         *   Those feature names are defined by the external crates and may change independently
         * - Cargo.toml feature names unless they are explicitly defined by us to match this value
         */
        const val HTTP_1X_CONFIG_KEY = "http-1x"

        private val KNOWN_CONFIG_KEYS =
            setOf(
                "formatTimeoutSeconds",
                "debugMode",
                "publicConstrainedTypes",
                "ignoreUnsupportedConstraints",
                "experimentalCustomValidationExceptionWithReasonPleaseDoNotUse",
                "addValidationExceptionToConstrainedOperations",
                "alwaysSendEventStreamInitialResponse",
                HTTP_1X_CONFIG_KEY,
            )

        fun fromCodegenConfigAndNode(
            coreCodegenConfig: CoreCodegenConfig,
            node: Optional<ObjectNode>,
        ) = if (node.isPresent) {
            // Validate that all config keys are known
            val configNode = node.get()
            val unknownKeys = configNode.members.keys.map { it.toString() }.filter { it !in KNOWN_CONFIG_KEYS }
            if (unknownKeys.isNotEmpty()) {
                throw IllegalArgumentException(
                    "Unknown codegen configuration key(s): ${unknownKeys.joinToString(", ")}. " +
                        "Known keys are: ${KNOWN_CONFIG_KEYS.joinToString(", ")}. ",
                )
            }

            ServerCodegenConfig(
                formatTimeoutSeconds = coreCodegenConfig.formatTimeoutSeconds,
                debugMode = coreCodegenConfig.debugMode,
                publicConstrainedTypes =
                    node.get()
                        .getBooleanMemberOrDefault("publicConstrainedTypes", DEFAULT_PUBLIC_CONSTRAINED_TYPES),
                ignoreUnsupportedConstraints =
                    node.get()
                        .getBooleanMemberOrDefault(
                            "ignoreUnsupportedConstraints",
                            DEFAULT_IGNORE_UNSUPPORTED_CONSTRAINTS,
                        ),
                experimentalCustomValidationExceptionWithReasonPleaseDoNotUse =
                    node.get().getStringMemberOrDefault(
                        "experimentalCustomValidationExceptionWithReasonPleaseDoNotUse",
                        defaultExperimentalCustomValidationExceptionWithReasonPleaseDoNotUse,
                    ),
                addValidationExceptionToConstrainedOperations =
                    node.get().getBooleanMemberOrDefault(
                        "addValidationExceptionToConstrainedOperations",
                        DEFAULT_ADD_VALIDATION_EXCEPTION_TO_CONSTRAINED_OPERATIONS,
                    ),
                alwaysSendEventStreamInitialResponse =
                    node.get().getBooleanMemberOrDefault(
                        "alwaysSendEventStreamInitialResponse",
                        DEFAULT_SEND_EVENT_STREAM_INITIAL_RESPONSE,
                    ),
                http1x =
                    node.get().getBooleanMemberOrDefault(
                        HTTP_1X_CONFIG_KEY,
                        DEFAULT_HTTP_1X,
                    ),
            )
        } else {
            ServerCodegenConfig(
                formatTimeoutSeconds = coreCodegenConfig.formatTimeoutSeconds,
                debugMode = coreCodegenConfig.debugMode,
            )
        }
    }
}
