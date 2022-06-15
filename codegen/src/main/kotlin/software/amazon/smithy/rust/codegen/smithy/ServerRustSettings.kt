package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.util.orNull
import java.util.*

// TODO from* functions are copied from RustSettings.kt

/**
 * [ServerRustSettings] and [ServerCodegenConfig] classes.
 *
 * These classes are entirely analogous to [ClientRustSettings] and [ClientCodegenConfig]. Refer to the documentation
 * for those.
 *
 * These classes have to live in the `codegen` subproject because they are referenced in [ServerCodegenContext],
 * which is used in common generators to both client and server (like [JsonParserGenerator]).
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
        fun from(model: Model, config: ObjectNode): ServerRustSettings {
            val codegenSettings = config.getObjectMember(CODEGEN_SETTINGS)
            val codegenConfig = ServerCodegenConfig.fromNode(codegenSettings)
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
        fun fromCodegenConfig(
            model: Model,
            config: ObjectNode,
            codegenConfig: ServerCodegenConfig
        ): ServerRustSettings {
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
            return ServerRustSettings(
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

data class ServerCodegenConfig(
    override val formatTimeoutSeconds: Int = 20,
    override val debugMode: Boolean = false,
    override val eventStreamAllowList: Set<String> = emptySet(),
): CodegenConfig(
    formatTimeoutSeconds, debugMode, eventStreamAllowList
) {
    companion object {
        // TODO It'd be nice if we could load the common properties and then just load the server ones here. Same for `ClientCodegenConfig`.
        fun fromNode(node: Optional<ObjectNode>): ServerCodegenConfig =
            if (node.isPresent) {
                ServerCodegenConfig(
                    node.get().getNumberMemberOrDefault("formatTimeoutSeconds", 20).toInt(),
                    node.get().getBooleanMemberOrDefault("debugMode", false),
                    node.get().getArrayMember("eventStreamAllowList")
                        .map { array -> array.toList().mapNotNull { node -> node.asStringNode().orNull()?.value } }
                        .orNull()?.toSet() ?: emptySet()
                )
            } else {
                ServerCodegenConfig()
            }
    }
}