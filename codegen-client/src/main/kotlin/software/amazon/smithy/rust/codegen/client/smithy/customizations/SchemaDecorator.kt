/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.generators.SchemaStructureCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureCustomization
import software.amazon.smithy.rust.codegen.core.util.dq

/**
 * Determines whether schema-based serialization/deserialization should be used
 * for a given codegen context. This controls both:
 * - Whether the schema path is the sole serialization path (no fallback to old codegen)
 * - Whether the old protocol_serde code is generated
 *
 * The allowlist supports two dimensions:
 * - Protocol trait IDs: all services using a given protocol are allowed
 * - Service shape IDs: specific services are allowed regardless of protocol
 *
 * During phased rollout, protocols/services can be added incrementally.
 * Once all protocols are listed, the allowlist can be removed entirely.
 */
object SchemaSerdeAllowlist {
    /** Protocols for which schema-based serde is the sole path (no fallback). */
    private val allowedProtocols: Set<ShapeId> =
        setOf(
            RestJson1Trait.ID,
            AwsJson1_0Trait.ID,
            AwsJson1_1Trait.ID,
        )

    /** Individual services allowed regardless of protocol. */
    private val allowedServices: Set<String> = setOf<String>()

    /** Returns true if schema-based serde should be used exclusively (no fallback). */
    fun usesSchemaSerdeExclusively(codegenContext: ClientCodegenContext): Boolean =
        codegenContext.protocol in allowedProtocols ||
            codegenContext.serviceShape.id.toString() in allowedServices
}

/**
 * Generates Schema implementations for all structure shapes and stores the
 * default protocol in the service config bag, enabling protocol-agnostic
 * serialization and deserialization.
 */
class SchemaDecorator : ClientCodegenDecorator {
    override val name: String = "SchemaDecorator"
    override val order: Byte = 0

    override fun structureCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<StructureCustomization>,
    ): List<StructureCustomization> {
        return baseCustomizations + SchemaStructureCustomization(codegenContext)
    }

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        if (SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
            baseCustomizations + SchemaProtocolCustomization(codegenContext)
        } else {
            baseCustomizations
        }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        if (SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
            baseCustomizations + SchemaOperationProtocolCustomization(codegenContext)
        } else {
            baseCustomizations
        }

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> =
        if (SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
            baseCustomizations + SchemaProtocolConfigCustomization(codegenContext)
        } else {
            baseCustomizations
        }
}

/**
 * Stores the default [SharedClientProtocol] in the service config bag
 * based on the protocol trait on the service shape.
 */
private class SchemaProtocolCustomization(
    private val codegenContext: ClientCodegenContext,
) : ServiceRuntimePluginCustomization() {
    override fun section(section: ServiceRuntimePluginSection) =
        writable {
            when (section) {
                is ServiceRuntimePluginSection.AdditionalConfig -> {
                    val smithyJson = CargoDependency.smithyJson(codegenContext.runtimeConfig).toType()
                    val smithySchema = RuntimeType.smithySchema(codegenContext.runtimeConfig)
                    val protocol = codegenContext.protocol
                    val serviceShapeName = codegenContext.serviceShape.id.name

                    val (protocolType, constructor) =
                        when {
                            protocol == RestJson1Trait.ID ->
                                smithyJson.resolve("protocol::aws_rest_json_1::AwsRestJsonProtocol") to "new()"
                            protocol == AwsJson1_0Trait.ID ->
                                smithyJson.resolve("protocol::aws_json_rpc::AwsJsonRpcProtocol") to "aws_json_1_0(${serviceShapeName.dq()})"
                            protocol == AwsJson1_1Trait.ID ->
                                smithyJson.resolve("protocol::aws_json_rpc::AwsJsonRpcProtocol") to "aws_json_1_1(${serviceShapeName.dq()})"
                            else -> return@writable // Other protocols not yet implemented
                        }

                    rustTemplate(
                        """
                        ${section.newLayerName}.store_put(
                            #{SharedClientProtocol}::new(#{ProtocolType}::$constructor)
                        );
                        """,
                        "SharedClientProtocol" to smithySchema.resolve("protocol::SharedClientProtocol"),
                        "ProtocolType" to protocolType,
                    )
                }
                else -> {}
            }
        }
}

/**
 * Stores the [SharedClientProtocol] in each operation's runtime plugin config layer,
 * so that the serializer and deserializer can access it without requiring the full
 * service config stack (e.g. in protocol tests and benchmarks).
 */
private class SchemaOperationProtocolCustomization(
    private val codegenContext: ClientCodegenContext,
) : OperationCustomization() {
    override fun section(section: OperationSection) =
        writable {
            when (section) {
                is OperationSection.AdditionalRuntimePluginConfig -> {
                    val smithyJson = CargoDependency.smithyJson(codegenContext.runtimeConfig).toType()
                    val smithySchema = RuntimeType.smithySchema(codegenContext.runtimeConfig)
                    val protocol = codegenContext.protocol
                    val serviceShapeName = codegenContext.serviceShape.id.name

                    val (protocolType, constructor) =
                        when {
                            protocol == RestJson1Trait.ID ->
                                smithyJson.resolve("protocol::aws_rest_json_1::AwsRestJsonProtocol") to "new()"
                            protocol == AwsJson1_0Trait.ID ->
                                smithyJson.resolve("protocol::aws_json_rpc::AwsJsonRpcProtocol") to "aws_json_1_0(${serviceShapeName.dq()})"
                            protocol == AwsJson1_1Trait.ID ->
                                smithyJson.resolve("protocol::aws_json_rpc::AwsJsonRpcProtocol") to "aws_json_1_1(${serviceShapeName.dq()})"
                            else -> return@writable
                        }

                    rustTemplate(
                        """
                        ${section.newLayerName}.store_put(
                            #{SharedClientProtocol}::new(#{ProtocolType}::$constructor)
                        );
                        """,
                        "SharedClientProtocol" to smithySchema.resolve("protocol::SharedClientProtocol"),
                        "ProtocolType" to protocolType,
                    )
                }
                else -> {}
            }
        }
}

/**
 * Adds protocol getter/setter to the service config builder, allowing
 * customers to override the default protocol at runtime.
 */
private class SchemaProtocolConfigCustomization(
    codegenContext: ClientCodegenContext,
) : ConfigCustomization() {
    private val smithySchema = RuntimeType.smithySchema(codegenContext.runtimeConfig)
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "ClientProtocol" to smithySchema.resolve("protocol::ClientProtocol"),
            "SharedClientProtocol" to smithySchema.resolve("protocol::SharedClientProtocol"),
        )

    override fun section(section: ServiceConfig): Writable =
        when (section) {
            is ServiceConfig.ConfigImpl ->
                writable {
                    rustTemplate(
                        """
                        /// Returns the client protocol used for serialization and deserialization.
                        pub fn protocol(&self) -> #{Option}<&#{SharedClientProtocol}> {
                            self.config.load::<#{SharedClientProtocol}>()
                        }
                        """,
                        *codegenScope,
                    )
                }

            ServiceConfig.BuilderImpl ->
                writable {
                    rustTemplate(
                        """
                        /// Sets the client protocol to use for serialization and deserialization.
                        ///
                        /// This overrides the default protocol determined by the service model,
                        /// enabling runtime protocol selection.
                        pub fn protocol(mut self, protocol: impl #{ClientProtocol} + 'static) -> Self {
                            self.set_protocol(#{Some}(#{SharedClientProtocol}::new(protocol)));
                            self
                        }

                        /// Sets the client protocol to use for serialization and deserialization.
                        pub fn set_protocol(&mut self, protocol: #{Option}<#{SharedClientProtocol}>) -> &mut Self {
                            self.config.store_or_unset(protocol);
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }

            is ServiceConfig.BuilderFromConfigBag ->
                writable {
                    rustTemplate(
                        """
                        if let #{Some}(protocol) = ${section.configBag}.load::<#{SharedClientProtocol}>().cloned() {
                            ${section.builder}.set_protocol(#{Some}(protocol));
                        }
                        """,
                        *codegenScope,
                    )
                }

            else -> emptySection
        }
}
