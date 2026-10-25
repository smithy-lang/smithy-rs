/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.HttpPayloadTrait
import software.amazon.smithy.rust.codegen.core.smithy.CODEGEN_SETTINGS
import software.amazon.smithy.rust.codegen.core.smithy.CoreCodegenConfig
import software.amazon.smithy.rust.codegen.core.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.core.smithy.HttpVersion
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import java.util.Optional
import java.util.logging.Logger
import kotlin.time.Duration
import kotlin.time.Duration.Companion.hours
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Duration.Companion.seconds

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
    val requestBodyReadTimeouts: RequestBodyReadTimeouts,
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
    private val parsedProtocolSettings by lazy { ProtocolReadSettings.protocolSettings(customizationConfig) }
    val headerReadTimeout: Duration? by lazy { ProtocolReadSettings.headerReadTimeout(protocolSettings()) }
    val requestBodyReadPollBudget: Int? by lazy { ProtocolReadSettings.requestBodyReadPollBudget(protocolSettings()) }
    val eventStreamTimeouts by lazy {
        EventStreamTimeouts.fromProtocolSettings(protocolSettings(), requestBodyReadTimeouts)
    }

    /** Normalized protocol sections, shared by every generated configuration reader. */
    fun protocolSettings(): Map<String, ObjectNode> = parsedProtocolSettings

    fun requestBodyMaxBytes(): Long? = ProtocolReadSettings.requestBodyMaxBytes(protocolSettings())

    fun requestBodyReadPollBudget(): Int? = requestBodyReadPollBudget

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

            val customizationConfig =
                ProtocolReadSettings.foldLegacyRequestBodyMaxBytes(
                    coreRustSettings.customizationConfig,
                    codegenSettingsNode.orElse(null),
                )
            val protocolSettings = ProtocolReadSettings.protocolSettings(customizationConfig)

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
                customizationConfig = customizationConfig,
                requestBodyReadTimeouts =
                    RequestBodyReadTimeouts.fromProtocolSettings(
                        model,
                        coreRustSettings.service,
                        protocolSettings,
                    ),
            ).also {
                it.headerReadTimeout
                it.requestBodyMaxBytes()
                it.requestBodyReadPollBudget()
                it.eventStreamTimeouts
            }
        }
    }
}

data class RequestBodyReadTimeouts(
    val defaultNonPayloadTimeout: Duration?,
    val defaultPayloadTimeout: Duration?,
    val perOperation: Map<ShapeId, Duration>,
    val payloadOperationIds: Set<ShapeId>,
    val streamingOperationIds: Set<ShapeId>,
    val eventStreamInputOperationIds: Set<ShapeId>,
) {
    /** Returns an override, preserving zero; null inherits runtime policy for non-streaming inputs. */
    fun timeoutFor(operationId: ShapeId): Duration? =
        if (operationId in streamingOperationIds) {
            null
        } else {
            perOperation[operationId]
                ?: if (operationId in payloadOperationIds) defaultPayloadTimeout else defaultNonPayloadTimeout
        }

    companion object {
        private const val CONFIG_KEY = "readTimeouts"
        private const val DEFAULT_NON_PAYLOAD_KEY = "defaultNonPayload"
        private const val DEFAULT_PAYLOAD_KEY = "defaultPayload"
        private const val PER_OPERATION_KEY = "perOperation"

        /** Parses a duration with an explicit unit, or `0` to disable the timeout. */
        internal fun parseTimeout(
            node: Node,
            configPath: String,
        ): Duration =
            when {
                node.isNumberNode && node.expectNumberNode().value.toDouble() == 0.0 -> Duration.ZERO
                node.isStringNode -> parseTimeoutString(node.expectStringNode().value, configPath)
                else -> throw invalidTimeout(configPath)
            }

        private fun parseTimeoutString(
            value: String,
            configPath: String,
        ): Duration {
            val trimmed = value.trim()
            val match =
                TIMEOUT_VALUE_REGEX.matchEntire(trimmed)
                    ?: throw invalidTimeout(configPath)
            return try {
                val amount = match.groupValues[1].toLong()
                val duration =
                    when (match.groupValues[2]) {
                        "ms" -> amount.milliseconds
                        "s" -> amount.seconds
                        "m" -> amount.minutes
                        "h" -> amount.hours
                        else -> throw invalidTimeout(configPath)
                    }
                if (duration.isInfinite()) {
                    throw CodegenException("`$configPath` exceeds the maximum supported duration")
                }
                duration
            } catch (_: NumberFormatException) {
                throw CodegenException("`$configPath` exceeds the maximum supported duration")
            }
        }

        private fun invalidTimeout(configPath: String) =
            CodegenException(
                "`$configPath` must be `0` to disable the timeout or a duration string with an explicit unit, " +
                    "such as `1000ms`, `10s`, `30m`, or `1h`",
            )

        private val TIMEOUT_VALUE_REGEX = Regex("^([0-9]+)\\s*(ms|s|m|h)$")

        fun fromCustomizationConfig(
            model: Model,
            serviceId: ShapeId,
            customizationConfig: ObjectNode?,
        ): RequestBodyReadTimeouts =
            fromProtocolSettings(model, serviceId, ProtocolReadSettings.protocolSettings(customizationConfig))

        fun fromProtocolSettings(
            model: Model,
            serviceId: ShapeId,
            protocolSettings: Map<String, ObjectNode>,
        ): RequestBodyReadTimeouts {
            val (configNode, configPath) = ProtocolReadSettings.resolve(protocolSettings, "request", "body", CONFIG_KEY)
            val config = ProtocolReadSettings.expectObject(configNode, configPath)
            config?.let {
                ProtocolReadSettings.warnUnknownKeys(
                    it,
                    setOf(DEFAULT_NON_PAYLOAD_KEY, DEFAULT_PAYLOAD_KEY, PER_OPERATION_KEY),
                    configPath,
                )
            }
            val defaultNonPayloadTimeout =
                config
                    ?.getMember(DEFAULT_NON_PAYLOAD_KEY)
                    ?.map { parseTimeout(it, "$configPath.$DEFAULT_NON_PAYLOAD_KEY") }
                    ?.orElse(null)
            val defaultPayloadTimeout =
                config
                    ?.getMember(DEFAULT_PAYLOAD_KEY)
                    ?.map { parseTimeout(it, "$configPath.$DEFAULT_PAYLOAD_KEY") }
                    ?.orElse(null)
            val service = model.expectShape(serviceId, ServiceShape::class.java)
            val containedOperations = TopDownIndex.of(model).getContainedOperations(service)
            val containedOperationIds =
                containedOperations
                    .map { it.id }
                    .toSet()
            val streamingOperationIds =
                containedOperations
                    .filter { operation ->
                        operation.input
                            .map { inputId ->
                                model.expectShape(inputId).members().any { member ->
                                    member.isStreaming(model)
                                }
                            }
                            .orElse(false)
                    }
                    .map { it.id }
                    .toSet()
            val eventStreamInputOperationIds =
                containedOperations
                    .filter { operation ->
                        operation.input
                            .map { inputId ->
                                model.expectShape(inputId).members().any { member ->
                                    member.isEventStream(model)
                                }
                            }
                            .orElse(false)
                    }
                    .map { it.id }
                    .toSet()
            val payloadOperationIds =
                containedOperations
                    .filter { operation ->
                        operation.id !in streamingOperationIds &&
                            operation.input
                                .map { inputId ->
                                    model.expectShape(inputId).members().any { member ->
                                        member.hasTrait<HttpPayloadTrait>()
                                    }
                                }
                                .orElse(false)
                    }
                    .map { it.id }
                    .toSet()
            val perOperation =
                ProtocolReadSettings.expectObject(
                    config?.getMember(PER_OPERATION_KEY)?.orElse(null),
                    "$configPath.$PER_OPERATION_KEY",
                )
                    ?.members
                    ?.map { (key, value) ->
                        val operationId =
                            try {
                                ShapeId.from(key.value)
                            } catch (error: RuntimeException) {
                                throw CodegenException(
                                    "`$configPath.$PER_OPERATION_KEY` contains invalid operation ID `${key.value}`",
                                    error,
                                )
                            }
                        if (operationId !in containedOperationIds) {
                            throw CodegenException(
                                "`$configPath.$PER_OPERATION_KEY` contains `$operationId`, " +
                                    "which is not an operation attached to service `$serviceId`",
                            )
                        }
                        // Streaming inputs have no whole-body read deadline. Event stream inputs are
                        // configured via `request.body.eventStreamTimeouts.perOperation` instead.
                        if (operationId in streamingOperationIds) {
                            val hint =
                                if (operationId in eventStreamInputOperationIds) {
                                    "; configure event stream deadlines via `${EventStreamTimeouts.CONFIG_PATH}.perOperation`"
                                } else {
                                    ""
                                }
                            throw CodegenException(
                                "`$configPath.$PER_OPERATION_KEY` contains streaming operation " +
                                    "`$operationId`, but request body read timeouts are not supported for streaming inputs$hint",
                            )
                        }
                        operationId to
                            parseTimeout(
                                value,
                                "$configPath.$PER_OPERATION_KEY.$operationId",
                            )
                    }?.toMap()
                    ?: emptyMap()

            return RequestBodyReadTimeouts(
                defaultNonPayloadTimeout,
                defaultPayloadTimeout,
                perOperation,
                payloadOperationIds,
                streamingOperationIds,
                eventStreamInputOperationIds,
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
 *   values). Omitted settings inherit the runtime default. Configured with
 *   `customizationConfig.protocols.common.request.body.maxBytes`; the legacy
 *   `codegen.requestBodyMaxBytes` key is read when the common key is absent.
 * [rpcV2CborAddCapitalizedRoute]: When false (default), the RPCv2 CBOR server router registers
 *   only the spec-compliant verbatim route derived from the Smithy operation shape name
 *   (e.g., `Example.getFoo`). When true, an additional legacy alias with the first character
 *   capitalized is also registered (e.g., `Example.GetFoo`) for operations whose Rust symbol
 *   name differs from the verbatim Smithy operation name. Set this to `true` to preserve
 *   compatibility with clients that were previously reaching the server via the capitalized URI
 *   (which was the only route the server registered before the fix for
 *   https://github.com/smithy-lang/smithy-rs/issues/4731).
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
    val requestBodyMaxBytes: Long? = null,
    /**
     * When true, a union JSON body whose object did not set any recognized variant
     * (e.g. `{}` or `{"unknownKey": ...}`) parses to `Ok(None)` rather than returning a
     * deserialization error. Off by default, opt in only for services that have shipped
     * clients depending on the lenient behavior.
     */
    val allowMissingUnionVariant: Boolean = DEFAULT_ALLOW_MISSING_UNION_VARIANT,
    val rpcV2CborAddCapitalizedRoute: Boolean = DEFAULT_RPC_V2_CBOR_ADD_CAPITALIZED_ROUTE,
) : CoreCodegenConfig(
        formatTimeoutSeconds, debugMode,
    ) {
    companion object {
        private const val DEFAULT_PUBLIC_CONSTRAINED_TYPES = true
        private const val DEFAULT_IGNORE_UNSUPPORTED_CONSTRAINTS = false
        private val defaultExperimentalCustomValidationExceptionWithReasonPleaseDoNotUse = null
        private const val DEFAULT_SEND_EVENT_STREAM_INITIAL_RESPONSE = false
        private const val DEFAULT_ALLOW_MISSING_UNION_VARIANT = false
        const val DEFAULT_HTTP_1X = false

        /**
         * Default value for `rpcV2CborAddCapitalizedRoute`.
         *
         * When false (default), the RPCv2 CBOR router registers only the spec-compliant verbatim
         * route. When true, an additional legacy capitalized alias is registered for operations
         * whose Rust symbol name differs from the verbatim Smithy operation name.
         */
        const val DEFAULT_RPC_V2_CBOR_ADD_CAPITALIZED_ROUTE = false

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

        /** Configuration key for the RPCv2 CBOR opt-in flag that adds a legacy capitalized route alias. */
        const val RPC_V2_CBOR_ADD_CAPITALIZED_ROUTE_CONFIG_KEY = "rpcV2CborAddCapitalizedRoute"

        private val KNOWN_CONFIG_KEYS =
            setOf(
                "formatTimeoutSeconds",
                "debugMode",
                "publicConstrainedTypes",
                "ignoreUnsupportedConstraints",
                "experimentalCustomValidationExceptionWithReasonPleaseDoNotUse",
                "addValidationExceptionToConstrainedOperations",
                "alwaysSendEventStreamInitialResponse",
                "allowMissingUnionVariant",
                HTTP_1X_CONFIG_KEY,
                REQUEST_BODY_MAX_BYTES_CONFIG_KEY,
                RPC_V2_CBOR_ADD_CAPITALIZED_ROUTE_CONFIG_KEY,
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
                allowMissingUnionVariant =
                    node.get().getBooleanMemberOrDefault(
                        "allowMissingUnionVariant",
                        DEFAULT_ALLOW_MISSING_UNION_VARIANT,
                    ),
                http1x =
                    node.get().getBooleanMemberOrDefault(
                        HTTP_1X_CONFIG_KEY,
                        DEFAULT_HTTP_1X,
                    ),
                requestBodyMaxBytes =
                    node.get().getNumberMember(REQUEST_BODY_MAX_BYTES_CONFIG_KEY)
                        .map { it.value.toLong() }.orElse(null),
                rpcV2CborAddCapitalizedRoute =
                    node.get().getBooleanMemberOrDefault(
                        RPC_V2_CBOR_ADD_CAPITALIZED_ROUTE_CONFIG_KEY,
                        DEFAULT_RPC_V2_CBOR_ADD_CAPITALIZED_ROUTE,
                    ),
            ).also {
                require(it.requestBodyMaxBytes == null || it.requestBodyMaxBytes >= 0) {
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
