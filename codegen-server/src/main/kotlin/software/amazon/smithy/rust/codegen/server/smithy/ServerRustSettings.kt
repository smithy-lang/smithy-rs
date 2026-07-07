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
import java.util.logging.Logger

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
 * [requestBodyMaxBytes]: Maximum number of bytes to buffer when deserializing a non-streaming
 *   request body. Set to `0` to disable the limit (the historical behavior; not recommended, as
 *   it allows memory exhaustion via `Transfer-Encoding: chunked` or very large `Content-Length`
 *   values). Default is `0` (no limit) for backwards compatibility.
 * [rpcV2CborUseVerbatimOperationName]: When true, the RPCv2 CBOR server router keys on the verbatim
 *   Smithy operation shape name (e.g., `getFoo`) per the smithy-rpc-v2 spec. When false (default),
 *   the router uses the PascalCased Rust symbol name (e.g., `GetFoo`), preserving the historical
 *   behavior. Set to true to fix client/server interoperability for operations with non-UpperCamelCase
 *   names (see https://github.com/smithy-lang/smithy-rs/issues/4731). Intended to become opt-out
 *   (default true) once downstream impact is validated.
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
    /**
     * @deprecated This flag is deprecated. `smithy.framework#ValidationException` is now automatically added to operations
     * with constrained inputs unless a custom validation exception (a structure with the `@validationException`
     * trait) is defined in the model. Setting this to false will disable the automatic addition, but this
     * behavior is deprecated and may be removed in a future release.
     */
    val addValidationExceptionToConstrainedOperations: Boolean? = null,
    val alwaysSendEventStreamInitialResponse: Boolean = DEFAULT_SEND_EVENT_STREAM_INITIAL_RESPONSE,
    val http1x: Boolean = DEFAULT_HTTP_1X,
    val requestBodyMaxBytes: Long = DEFAULT_REQUEST_BODY_MAX_BYTES,
    val rpcV2CborUseVerbatimOperationName: Boolean = DEFAULT_RPC_V2_CBOR_USE_VERBATIM_OPERATION_NAME,
) : CoreCodegenConfig(
        formatTimeoutSeconds, debugMode,
    ) {
    companion object {
        private const val DEFAULT_PUBLIC_CONSTRAINED_TYPES = true
        private const val DEFAULT_IGNORE_UNSUPPORTED_CONSTRAINTS = false
        private val defaultExperimentalCustomValidationExceptionWithReasonPleaseDoNotUse = null
        private const val DEFAULT_SEND_EVENT_STREAM_INITIAL_RESPONSE = false
        const val DEFAULT_HTTP_1X = false

        /**
         * The default maximum size (in bytes) of a non-streaming request body that the generated
         * server will buffer into memory. `0` means no limit (the historical behavior).
         *
         * Services should set `requestBodyMaxBytes` to a positive value to prevent
         * memory-exhaustion denial-of-service attacks via unbounded request bodies.
         */
        const val DEFAULT_REQUEST_BODY_MAX_BYTES: Long = 0L

        /**
         * Default value for `rpcV2CborUseVerbatimOperationName`.
         *
         * When false (default), the RPCv2 CBOR router uses PascalCased Rust symbol names as keys,
         * preserving historical behavior. When true, it uses verbatim Smithy operation names per spec.
         */
        const val DEFAULT_RPC_V2_CBOR_USE_VERBATIM_OPERATION_NAME = false

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

        /** Configuration key for the per-request body size limit. */
        const val REQUEST_BODY_MAX_BYTES_CONFIG_KEY = "requestBodyMaxBytes"

        /** Configuration key for the RPCv2 CBOR verbatim operation name flag. */
        const val RPC_V2_CBOR_USE_VERBATIM_OPERATION_NAME_CONFIG_KEY = "rpcV2CborUseVerbatimOperationName"

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
                REQUEST_BODY_MAX_BYTES_CONFIG_KEY,
                RPC_V2_CBOR_USE_VERBATIM_OPERATION_NAME_CONFIG_KEY,
            )

        fun fromCodegenConfigAndNode(
            coreCodegenConfig: CoreCodegenConfig,
            node: Optional<ObjectNode>,
        ) = if (node.isPresent) {
            // Validate that all config keys are known
            val configNode = node.get()
            val unknownKeys = configNode.members.keys.map { it.toString() }.filter { it !in KNOWN_CONFIG_KEYS }
            if (unknownKeys.isNotEmpty()) {
                Logger.getLogger("ServerCodegenConfig").warning(
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
                    node.get().getBooleanMember(
                        "addValidationExceptionToConstrainedOperations",
                    ).orElse(null)?.value,
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
                requestBodyMaxBytes =
                    node.get().getNumberMemberOrDefault(
                        REQUEST_BODY_MAX_BYTES_CONFIG_KEY,
                        DEFAULT_REQUEST_BODY_MAX_BYTES,
                    ).toLong(),
                rpcV2CborUseVerbatimOperationName =
                    node.get().getBooleanMemberOrDefault(
                        RPC_V2_CBOR_USE_VERBATIM_OPERATION_NAME_CONFIG_KEY,
                        DEFAULT_RPC_V2_CBOR_USE_VERBATIM_OPERATION_NAME,
                    ),
            ).also {
                require(it.requestBodyMaxBytes >= 0) {
                    "`$REQUEST_BODY_MAX_BYTES_CONFIG_KEY` must be non-negative, got ${it.requestBodyMaxBytes}"
                }
            }
        } else {
            ServerCodegenConfig(
                formatTimeoutSeconds = coreCodegenConfig.formatTimeoutSeconds,
                debugMode = coreCodegenConfig.debugMode,
            )
        }
    }
}
