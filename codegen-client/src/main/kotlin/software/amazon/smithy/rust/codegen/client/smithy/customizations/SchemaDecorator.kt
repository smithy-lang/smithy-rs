/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.AwsQueryTrait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.XmlNamespaceTrait
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.generators.SchemaStructureCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureCustomization
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait

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
    /**
     * Protocols for which schema-based serde is the sole path (no fallback).
     */
    private val allowedProtocols: Set<ShapeId> = emptySet()

    /**
     * Individual services allowed regardless of protocol.
     *
     * We should uncomment the test models as we enable protocols
     */
    private val allowedServices: Set<String> =
        setOf(
            // --- Phased rollout: real AWS services ---
            // Enabled one service at a time, ahead of enabling a whole protocol via
            // `allowedProtocols`. A service entry only affects that service, so the
            // protocol's other services (e.g. `config` for awsJson1_1) stay on the
            // legacy path and keep acting as a control.
            //
            // awsJson1_1 — first service on the schema path. Its model lives at
            // `aws/sdk/aws-models/ssm.json` so CI generates and tests it.
            "com.amazonaws.ssm#AmazonSSM",
            // Test model names, listed explicitly until protocols are fully enabled
            // restJson1
            // "aws.protocoltests.restjson#RestJson",
            // "aws.protocoltests.restjson#RestJsonExtras",
            // "aws.protocoltests.misc#MiscService",
            // "com.aws.example#PokemonService",
            // "com.amazonaws.ebs#Ebs",
            // awsJson1_0 / awsJson1_1
            // "aws.protocoltests.json10#JsonRpc10",
            // "aws.protocoltests.json#JsonProtocol",
            // "aws.protocoltests.json#TestService",
            // "aws.protocoltests.misc#QueryCompatService",
            // "com.amazonaws.simple#SimpleService",
            // "com.amazonaws.bignumbers#BigNumberService",
            // restXml
            // "aws.protocoltests.restxml#RestXml",
            // "aws.protocoltests.restxml#RestXmlExtras",
            // "aws.protocoltests.restxml.xmlns#RestXmlWithNamespace",
            // "aws.protocoltests.restxmlunwrapped#RestXmlExtrasUnwrappedErrors",
            // rpcv2Cbor
            // "smithy.protocoltests.rpcv2Cbor#RpcV2Protocol",
            // "smithy.protocoltests.rpcv2Cbor#RpcV2CborService",
            // "aws.protocoltests.rpcv2cbor#QueryCompatibleRpcV2Protocol",
            // "aws.protocoltests.rpcv2cbor#NonQueryCompatibleRpcV2Protocol",
            // naming obstacle courses (protocol-independent codegen coverage)
            // "crate#Config",
            // "casing#ACRONYMInside_Service",
            // "naming_obs_structs#NamingObstacleCourseStructs",
        )

    /**
     * Returns true if schema-based serde should be used exclusively (no fallback).
     *
     * The `disableSchemaSerde` codegen setting overrides the allowlist in one direction only: a
     * service can opt *out* of schema serde in its `smithy-build.json` even when its protocol is
     * allowlisted, but it cannot opt in. See `ClientCodegenConfig.disableSchemaSerde`.
     */
    fun usesSchemaSerdeExclusively(codegenContext: ClientCodegenContext): Boolean =
        !codegenContext.settings.codegenConfig.disableSchemaSerde &&
            (
                codegenContext.protocol in allowedProtocols ||
                    codegenContext.serviceShape.id.toString() in allowedServices
            )

    /**
     * Returns true if schema-based serde is enabled for [protocol].
     *
     * Tests that exercise schema-serde-only generated APIs (e.g. the type/error
     * registries or the schema-serde streaming deserializer) gate themselves on
     * this so they run exactly when the corresponding protocol is enabled and are
     * skipped otherwise, rather than being hard-disabled.
     *
     * This reports the allowlist state only; it does not account for the per-service
     * `disableSchemaSerde` codegen setting, which needs a full codegen context to evaluate.
     */
    fun isProtocolEnabled(protocol: ShapeId): Boolean = protocol in allowedProtocols
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
    ): List<StructureCustomization> =
        if (SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
            baseCustomizations + SchemaStructureCustomization(codegenContext)
        } else {
            baseCustomizations
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
                    val smithyJson = RuntimeType.smithyJson(codegenContext.runtimeConfig)
                    val smithyCbor = RuntimeType.smithyCbor(codegenContext.runtimeConfig)
                    val smithySchema = RuntimeType.smithySchema(codegenContext.runtimeConfig)
                    val protocol = codegenContext.protocol
                    val serviceShapeName = codegenContext.serviceShape.id.name
                    val serviceNamespace = codegenContext.serviceShape.id.namespace

                    // Stored unconditionally, not just for the protocol this client was generated
                    // for. Protocols whose wire format depends on model facts — rpcv2Cbor routes to
                    // `/service/{service}/operation/{operation}`, awsJson prefixes `X-Amz-Target`
                    // with the service shape name, awsQuery sends `Version=`, restXml applies the
                    // service `@xmlNamespace` as the root xmlns, and the JSON protocols resolve
                    // relative `__type` document discriminators against the service's shape-ID
                    // namespace — need those facts at runtime, and
                    // `Config::builder().protocol(..)` lets a customer plug in such a protocol
                    // regardless of what the model declared. A customer cannot supply them because
                    // only the model knows them.
                    //
                    // Kept as separate entries rather than one aggregate: `ConfigBag` is
                    // `TypeId`-keyed, so each protocol loads exactly what it needs and no shared
                    // struct accretes protocol-specific fields. They are service-scoped and stored
                    // once per client here, unlike `Metadata`, which is operation-scoped and
                    // re-emitted per operation.
                    // See https://github.com/smithy-lang/smithy-rs/issues/4801.
                    //
                    // `ServiceShapeNamespace` is the namespace half of the service's shape ID and
                    // is always present, so unlike `ServiceXmlNamespace` below it is unconditional.
                    // The two are unrelated values: a service's shape-ID namespace is not derivable
                    // from its `@xmlNamespace` URI or vice versa (CloudWatch Logs is
                    // `com.amazonaws.cloudwatchlogs` but declares
                    // `http://monitoring.amazonaws.com/doc/2014-03-28/`).
                    rustTemplate(
                        """
                        ${section.newLayerName}.store_put(#{ServiceShapeName}::new(${serviceShapeName.dq()}));
                        ${section.newLayerName}.store_put(#{ServiceShapeNamespace}::new(${serviceNamespace.dq()}));
                        ${section.newLayerName}.store_put(#{ServiceVersion}::new(${codegenContext.serviceShape.version.dq()}));
                        """,
                        "ServiceShapeName" to smithySchema.resolve("protocol::ServiceShapeName"),
                        "ServiceShapeNamespace" to smithySchema.resolve("protocol::ServiceShapeNamespace"),
                        "ServiceVersion" to smithySchema.resolve("protocol::ServiceVersion"),
                    )

                    // `@xmlNamespace` is a prelude trait, so it is resolvable from any model rather
                    // than only from a restXml one — but it is optional, so there is nothing to
                    // store when the service does not declare it.
                    codegenContext.serviceShape.getTrait(XmlNamespaceTrait::class.java).orElse(null)
                        ?.let { ns ->
                            val prefix =
                                ns.prefix.orElse(null)?.let { "Some(${it.dq()}.into())" } ?: "None"
                            rustTemplate(
                                """
                                ${section.newLayerName}.store_put(#{ServiceXmlNamespace}::new(${ns.uri.dq()}, $prefix));
                                """,
                                "ServiceXmlNamespace" to smithySchema.resolve("protocol::ServiceXmlNamespace"),
                            )
                        }

                    val (protocolType, constructor) =
                        when {
                            protocol == RestJson1Trait.ID ->
                                smithyJson.resolve("protocol::aws_rest_json_1::AwsRestJsonProtocol") to
                                    "new().with_default_namespace(${serviceNamespace.dq()})"
                            protocol == AwsJson1_0Trait.ID ->
                                smithyJson.resolve("protocol::aws_json_rpc::AwsJsonRpcProtocol") to
                                    "aws_json_1_0().with_default_namespace(${serviceNamespace.dq()})"
                            protocol == AwsJson1_1Trait.ID ->
                                smithyJson.resolve("protocol::aws_json_rpc::AwsJsonRpcProtocol") to
                                    "aws_json_1_1().with_default_namespace(${serviceNamespace.dq()})"
                            protocol == RestXmlTrait.ID -> {
                                val smithyXml = RuntimeType.smithyXml(codegenContext.runtimeConfig)
                                val noWrap = codegenContext.serviceShape.expectTrait<RestXmlTrait>().isNoErrorWrapping
                                val builderChain = StringBuilder("new()")
                                // `noErrorWrapping` has no config-bag default and cannot have one:
                                // it lives on the `@restXml` trait, which a non-restXml model does
                                // not carry. The service `@xmlNamespace` is resolved from the bag.
                                if (noWrap) builderChain.append(".with_no_error_wrapping(true)")
                                smithyXml.resolve("protocol::aws_rest_xml::AwsRestXmlProtocol") to builderChain.toString()
                            }
                            protocol == AwsQueryTrait.ID -> {
                                val smithyQuery = RuntimeType.smithyQuery(codegenContext.runtimeConfig)
                                smithyQuery.resolve("protocol::AwsQueryProtocol") to "new()"
                            }
                            protocol == Rpcv2CborTrait.ID ->
                                smithyCbor.resolve("protocol::RpcV2CborProtocol") to "new()"
                            else -> return@writable // Other protocols not yet implemented
                        }

                    rustTemplate(
                        """
                        if ${section.serviceConfigName}.protocol().is_none() {
                            ${section.newLayerName}.store_put(
                                #{SharedClientProtocol}::new(#{ProtocolType}::$constructor)
                            );
                        }
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
                        ///
                        /// ## Transport
                        ///
                        /// This setter is HTTP-specific. The config bag stores
                        /// `SharedClientProtocol` (which elides to its HTTP specialization) and
                        /// only `SharedClientProtocol<http::Request, http::Response>` has a
                        /// `Storable` impl. The `impl ClientProtocol + 'static` bound here elides
                        /// to `impl ClientProtocol<http::Request, http::Response>` to match —
                        /// a `ClientProtocol<Other, Other>` impl wouldn't round-trip through
                        /// config-bag storage even though the trait itself is transport-generic.
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
