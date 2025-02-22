/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.ToNode
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AbstractTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.util.runCommand
import java.io.File
import java.nio.file.Path
import java.util.logging.Logger

/**
 * A helper class holding common data with defaults that is threaded through several functions, to make their
 * signatures shorter.
 */
data class IntegrationTestParams(
    val addModuleToEventStreamAllowList: Boolean = false,
    val service: String? = null,
    val moduleVersion: String = "1.0.0",
    val runtimeConfig: RuntimeConfig? = null,
    val additionalSettings: ObjectNode = ObjectNode.builder().build(),
    val overrideTestDir: File? = null,
    val command: ((Path) -> Unit)? = null,
    val cargoCommand: String? = null,
)

/**
 * A helper class to allow setting `codegen` object keys to be passed to the `additionalSettings`
 * field of `IntegrationTestParams`.
 *
 * Usage:
 *
 * ```kotlin
 * serverIntegrationTest(
 *     model,
 *     IntegrationTestParams(
 *         additionalSettings =
 *             ServerAdditionalSettings.builder()
 *                 .generateCodegenComments()
 *                 .publicConstrainedTypes()
 *                 .toObjectNode()
 * )),
 * ```
 */
sealed class AdditionalSettings {
    abstract fun toObjectNode(): ObjectNode

    companion object {
        private fun Map<String, Any>.toCodegenObjectNode(): ObjectNode =
            ObjectNode.builder()
                .withMember(
                    "codegen",
                    ObjectNode.builder().apply {
                        forEach { (key, value) ->
                            when (value) {
                                is Boolean -> withMember(key, value)
                                is Number -> withMember(key, value)
                                is String -> withMember(key, value)
                                is ToNode -> withMember(key, value)
                                else -> throw IllegalArgumentException("Unsupported type for key $key: ${value::class}")
                            }
                        }
                    }.build(),
                )
                .build()
    }

    abstract class CoreAdditionalSettings protected constructor(
        private val settings: Map<String, Any>,
    ) : AdditionalSettings() {
        override fun toObjectNode(): ObjectNode = settings.toCodegenObjectNode()

        abstract class Builder<T : CoreAdditionalSettings> : AdditionalSettings() {
            protected val settings = mutableMapOf<String, Any>()

            fun generateCodegenComments(debugMode: Boolean = true) =
                apply {
                    settings["debugMode"] = debugMode
                }

            override fun toObjectNode(): ObjectNode = settings.toCodegenObjectNode()
        }
    }
}

class ServerAdditionalSettings private constructor(
    settings: Map<String, Any>,
) : AdditionalSettings.CoreAdditionalSettings(settings) {
    class Builder : CoreAdditionalSettings.Builder<ServerAdditionalSettings>() {
        fun publicConstrainedTypes(enabled: Boolean = true) =
            apply {
                settings["publicConstrainedTypes"] = enabled
            }

        fun addValidationExceptionToConstrainedOperations(enabled: Boolean = true) =
            apply {
                settings["addValidationExceptionToConstrainedOperations"] = enabled
            }

        fun replaceInvalidUtf8(enabled: Boolean = true) =
            apply {
                settings["replaceInvalidUtf8"] = enabled
            }
    }

    companion object {
        fun builder() = Builder()
    }
}

class ClientAdditionalSettings private constructor(
    settings: Map<String, Any>,
) : AdditionalSettings.CoreAdditionalSettings(settings) {
    class Builder : CoreAdditionalSettings.Builder<ClientAdditionalSettings>()

    companion object {
        fun builder() = Builder()
    }
}

/**
 * Run cargo test on a true, end-to-end, codegen product of a given model.
 */
fun codegenIntegrationTest(
    model: Model,
    params: IntegrationTestParams,
    invokePlugin: (PluginContext) -> Unit,
    environment: Map<String, String> = mapOf(),
): Path {
    val (ctx, testDir) =
        generatePluginContext(
            model,
            params.additionalSettings,
            params.addModuleToEventStreamAllowList,
            params.moduleVersion,
            params.service,
            params.runtimeConfig,
            params.overrideTestDir,
        )

    testDir.writeDotCargoConfigToml(listOf("--deny", "warnings"))

    invokePlugin(ctx)
    ctx.fileManifest.printGeneratedFiles()
    val logger = Logger.getLogger("CodegenIntegrationTest")
    val out = params.command?.invoke(testDir) ?: (params.cargoCommand ?: "cargo test --lib --tests").runCommand(testDir, environment = environment)
    logger.fine(out.toString())
    return testDir
}

/**
 * Metadata associated with a protocol that provides additional information needed for testing.
 *
 * @property protocol The protocol enum value this metadata is associated with
 * @property contentType The HTTP Content-Type header value associated with this protocol.
 */
data class ProtocolMetadata(
    val protocol: ModelProtocol,
    val contentType: String,
)

/**
 * Represents the supported protocol traits in Smithy models.
 *
 * @property trait The Smithy trait instance with which the service shape must be annotated.
 */
enum class ModelProtocol(val trait: AbstractTrait) {
    AwsJson10(AwsJson1_0Trait.builder().build()),
    AwsJson11(AwsJson1_1Trait.builder().build()),
    RestJson(RestJson1Trait.builder().build()),
    RestXml(RestXmlTrait.builder().build()),
    Rpcv2Cbor(Rpcv2CborTrait.builder().build()),
    ;

    // Create metadata after enum is initialized
    val metadata: ProtocolMetadata by lazy {
        when (this) {
            AwsJson10 -> ProtocolMetadata(this, "application/x-amz-json-1.0")
            AwsJson11 -> ProtocolMetadata(this, "application/x-amz-json-1.1")
            RestJson -> ProtocolMetadata(this, "application/json")
            RestXml -> ProtocolMetadata(this, "application/xml")
            Rpcv2Cbor -> ProtocolMetadata(this, "application/cbor")
        }
    }

    companion object {
        private val TRAIT_IDS = values().map { it.trait.toShapeId() }.toSet()
        val ALL: Set<ModelProtocol> = values().toSet()

        fun getTraitIds() = TRAIT_IDS
    }
}

/**
 * Removes all existing protocol traits annotated on the given service,
 * then sets the provided `protocol` as the sole protocol trait for the service.
 */
fun Model.replaceProtocolTraitOnServerShapeId(
    serviceShapeId: ShapeId,
    modelProtocol: ModelProtocol,
): Model {
    val serviceShape = this.expectShape(serviceShapeId, ServiceShape::class.java)
    return replaceProtocolTraitOnServiceShape(serviceShape, modelProtocol)
}

/**
 * Removes all existing protocol traits annotated on the given service shape,
 * then sets the provided `protocol` as the sole protocol trait for the service.
 */
fun Model.replaceProtocolTraitOnServiceShape(
    serviceShape: ServiceShape,
    modelProtocol: ModelProtocol,
): Model {
    val serviceBuilder = serviceShape.toBuilder()
    ModelProtocol.getTraitIds().forEach { traitId ->
        serviceBuilder.removeTrait(traitId)
    }
    val service = serviceBuilder.addTrait(modelProtocol.trait).build()
    return ModelTransformer.create().replaceShapes(this, listOf(service))
}

/**
 * Processes a Smithy model string by applying different protocol traits and invoking the tests block on the model.
 * For each protocol, this function:
 *  1. Parses the Smithy model string
 *  2. Replaces any existing protocol traits on service shapes with the specified protocol
 *  3. Runs the provided test with the transformed model and protocol metadata
 *
 * @param protocolTraitIds Set of protocols to test against
 * @param test Function that receives the transformed model and protocol metadata for testing
 */
fun String.forProtocols(
    protocolTraitIds: Set<ModelProtocol>,
    test: (Model, ProtocolMetadata) -> Unit,
) {
    val baseModel = this.asSmithyModel(smithyVersion = "2")
    val serviceShapes = baseModel.serviceShapes.toList()

    protocolTraitIds.forEach { protocol ->
        val transformedModel =
            serviceShapes.fold(baseModel) { acc, shape ->
                acc.replaceProtocolTraitOnServiceShape(shape, protocol)
            }
        test(transformedModel, protocol.metadata)
    }
}

/**
 * Convenience overload that accepts vararg protocols instead of a Set.
 *
 * @param protocols Variable number of protocols to test against
 * @param test Function that receives the transformed model and protocol metadata for testing
 * @see forProtocols
 */
fun String.forProtocols(
    vararg protocols: ModelProtocol,
    test: (Model, ProtocolMetadata) -> Unit,
) {
    forProtocols(protocols.toSet(), test)
}

/**
 * Tests a Smithy model string against all supported protocols, with optional exclusions.
 *
 * @param exclude Set of protocols to exclude from testing (default is empty)
 * @param test Function that receives the transformed model and protocol metadata for testing
 * @see forProtocols
 */
fun String.forAllProtocols(
    exclude: Set<ModelProtocol> = emptySet(),
    test: (Model, ProtocolMetadata) -> Unit,
) {
    forProtocols(ModelProtocol.ALL - exclude, test)
}
