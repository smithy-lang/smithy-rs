/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
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

/**
 * Generates Schema implementations for all structure shapes and stores the
 * default protocol in the service config bag, enabling protocol-agnostic
 * serialization and deserialization.
 */
class SchemaDecorator : ClientCodegenDecorator {
    override val name: String = "SchemaDecorator"
    override val order: Byte = 0

    // Uncomment the following to limit schema generation to specific services
    // during phased rollout. When the list is empty or this is commented out,
    // schemas are generated for all services.
    //
    // private val allowedServices = setOf(
    //     "com.amazonaws.dynamodb#DynamoDB_20120810",
    //     "com.amazonaws.sqs#AmazonSQS",
    //     "com.amazonaws.s3#AmazonS3",
    // )
    //
    // private fun isEnabled(codegenContext: ClientCodegenContext): Boolean =
    //     allowedServices.contains(codegenContext.serviceShape.id.toString())

    override fun structureCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<StructureCustomization>,
    ): List<StructureCustomization> {
        // if (!isEnabled(codegenContext)) return baseCustomizations
        return baseCustomizations + SchemaStructureCustomization(codegenContext)
    }

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> = baseCustomizations + SchemaProtocolCustomization(codegenContext)

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations + SchemaProtocolConfigCustomization(codegenContext)
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

                    val (protocolType, constructor) =
                        when {
                            protocol == RestJson1Trait.ID ->
                                smithyJson.resolve("protocol::aws_rest_json_1::AwsRestJsonProtocol") to "new()"
                            protocol == AwsJson1_0Trait.ID ->
                                smithyJson.resolve("protocol::aws_json_rpc::AwsJsonRpcProtocol") to "aws_json_1_0()"
                            protocol == AwsJson1_1Trait.ID ->
                                smithyJson.resolve("protocol::aws_json_rpc::AwsJsonRpcProtocol") to "aws_json_1_1()"
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
                        "${section.builder}.set_protocol(${section.configBag}.load::<#{SharedClientProtocol}>().cloned());",
                        *codegenScope,
                    )
                }

            else -> emptySection
        }
}
