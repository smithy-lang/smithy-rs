/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import kotlin.io.path.readText

class AwsEndpointDecorator : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    override val name: String = "AwsEndpoint"
    override val order: Byte = 0

    private var endpointsCache: ObjectNode? = null

    private fun endpoints(sdkSettings: SdkSettings): ObjectNode {
        if (endpointsCache == null) {
            val endpointsJson = when (val path = sdkSettings.endpointsConfigPath) {
                null -> (
                    javaClass.getResource("/default-sdk-endpoints.json")
                        ?: throw IllegalStateException("Failed to find default-sdk-endpoints.json in the JAR")
                    ).readText()
                else -> path.readText()
            }
            endpointsCache = Node.parse(endpointsJson).expectObjectNode()
        }
        return endpointsCache!!
    }

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations + EndpointConfigCustomization(
            codegenContext,
            endpoints(SdkSettings.from(codegenContext.settings)),
        )
    }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations + EndpointResolverFeature(codegenContext.runtimeConfig)
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> {
        return baseCustomizations + PubUseEndpoint(codegenContext.runtimeConfig)
    }

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}

class EndpointConfigCustomization(
    private val codegenContext: CodegenContext,
    private val endpointData: ObjectNode,
) :
    ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val moduleUseName = codegenContext.moduleUseName()
    private val codegenScope = arrayOf(
        "SmithyResolver" to RuntimeType.smithyHttp(runtimeConfig).resolve("endpoint::ResolveEndpoint"),
        "PlaceholderParams" to AwsRuntimeType.awsEndpoint(runtimeConfig).resolve("Params"),
        "ResolveAwsEndpoint" to AwsRuntimeType.awsEndpoint(runtimeConfig).resolve("ResolveAwsEndpoint"),
        "EndpointShim" to AwsRuntimeType.awsEndpoint(runtimeConfig).resolve("EndpointShim"),
        "aws_types" to AwsRuntimeType.awsTypes(runtimeConfig),
    )

    override fun section(section: ServiceConfig): Writable = writable {
        when (section) {
            is ServiceConfig.ConfigStruct -> rustTemplate(
                "pub (crate) endpoint_resolver: std::sync::Arc<dyn #{SmithyResolver}<#{PlaceholderParams}>>,",
                *codegenScope,
            )
            is ServiceConfig.ConfigImpl -> emptySection
// TODO(https://github.com/awslabs/smithy-rs/issues/1780): Uncomment once endpoints 2.0 project is completed
//                rustTemplate(
//                """
//                /// Returns the endpoint resolver.
//                pub fn endpoint_resolver(&self) -> std::sync::Arc<dyn #{SmithyResolver}<#{PlaceholderParams}>> {
//                    self.endpoint_resolver.clone()
//                }
//                """,
//                *codegenScope,
//            )
            is ServiceConfig.BuilderStruct ->
                rustTemplate("endpoint_resolver: Option<std::sync::Arc<dyn #{SmithyResolver}<#{PlaceholderParams}>>>,", *codegenScope)
            ServiceConfig.BuilderImpl ->
                rustTemplate(
                    """
                    /// Overrides the endpoint resolver to use when making requests.
                    ///
                    /// When unset, the client will used a generated endpoint resolver based on the endpoint metadata
                    /// for `$moduleUseName`.
                    ///
                    /// ## Examples
                    /// ```no_run
                    /// ## fn wrapper() -> Result<(), aws_smithy_http::endpoint::error::InvalidEndpointError> {
                    /// use #{aws_types}::region::Region;
                    /// use $moduleUseName::config::{Builder, Config};
                    /// use $moduleUseName::Endpoint;
                    ///
                    /// let config = $moduleUseName::Config::builder()
                    ///     .endpoint_resolver(Endpoint::immutable("http://localhost:8080")?)
                    ///     .build();
                    /// ## Ok(())
                    /// ## }
                    /// ```
                    pub fn endpoint_resolver(mut self, endpoint_resolver: impl #{ResolveAwsEndpoint} + 'static) -> Self {
                        self.endpoint_resolver = Some(std::sync::Arc::new(#{EndpointShim}::from_resolver(endpoint_resolver)) as _);
                        self
                    }

                    /// Sets the endpoint resolver to use when making requests.
                    pub fn set_endpoint_resolver(&mut self, endpoint_resolver: Option<std::sync::Arc<dyn #{ResolveAwsEndpoint}>>) -> &mut Self {
                        self.endpoint_resolver = endpoint_resolver.map(|res|std::sync::Arc::new(#{EndpointShim}::from_arc(res) ) as _);
                        self
                    }
                    """,
                    *codegenScope,
                )

            ServiceConfig.BuilderBuild -> {
                val resolverGenerator = EndpointResolverGenerator(codegenContext, endpointData)
                rustTemplate(
                    """
                    endpoint_resolver: self.endpoint_resolver.unwrap_or_else(||
                        std::sync::Arc::new(#{EndpointShim}::from_resolver(#{Resolver}()))
                    ),
                    """,
                    *codegenScope, "Resolver" to resolverGenerator.resolver(),
                )
            }

            else -> emptySection
        }
    }
}

class EndpointResolverFeature(runtimeConfig: RuntimeConfig) :
    OperationCustomization() {
    private val placeholderEndpointParams = AwsRuntimeType.awsEndpoint(runtimeConfig).resolve("Params")
    private val codegenScope = arrayOf(
        "PlaceholderParams" to placeholderEndpointParams,
        "BuildError" to runtimeConfig.operationBuildError(),
    )
    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateRequest -> writable {
                // insert the endpoint resolution _result_ into the bag (note that this won't bail if endpoint resolution failed)
                rustTemplate(
                    """
                    let endpoint_params = #{PlaceholderParams}::new(${section.config}.region.clone());
                    ${section.request}.properties_mut()
                        .insert::<aws_smithy_http::endpoint::Result>(
                            ${section.config}.endpoint_resolver.resolve_endpoint(&endpoint_params)
                        );
                    """,
                    *codegenScope,
                )
            }
            else -> emptySection
        }
    }
}

class PubUseEndpoint(private val runtimeConfig: RuntimeConfig) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable {
        return when (section) {
            is LibRsSection.Body -> writable {
                rust(
                    "pub use #T::endpoint::Endpoint;",
                    CargoDependency.smithyHttp(runtimeConfig).toType(),
                )
            }
            else -> emptySection
        }
    }
}

class EndpointResolverGenerator(codegenContext: CodegenContext, private val endpointData: ObjectNode) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val endpointPrefix = codegenContext.serviceShape.expectTrait<ServiceTrait>().endpointPrefix
    private val awsEndpoint = AwsRuntimeType.awsEndpoint(runtimeConfig)
    private val awsTypes = AwsRuntimeType.awsTypes(runtimeConfig)
    private val codegenScope =
        arrayOf(
            "Partition" to awsEndpoint.resolve("Partition"),
            "endpoint" to awsEndpoint.resolve("partition::endpoint"),
            "CredentialScope" to awsEndpoint.resolve("CredentialScope"),
            "Regionalized" to awsEndpoint.resolve("partition::Regionalized"),
            "Protocol" to awsEndpoint.resolve("partition::endpoint::Protocol"),
            "SignatureVersion" to awsEndpoint.resolve("partition::endpoint::SignatureVersion"),
            "PartitionResolver" to awsEndpoint.resolve("PartitionResolver"),
            "ResolveAwsEndpoint" to awsEndpoint.resolve("ResolveAwsEndpoint"),
            "SigningService" to awsTypes.resolve("SigningService"),
            "SigningRegion" to awsTypes.resolve("region::SigningRegion"),
        )

    fun resolver(): RuntimeType {
        val partitionsData = endpointData.expectArrayMember("partitions").getElementsAs(Node::expectObjectNode)

        val partitions = partitionsData.map {
            PartitionNode(endpointPrefix, it)
        }.sortedWith { x, y ->
            // always put the aws constructor first
            if (x.id == "aws") {
                -1
            } else {
                x.id.compareTo(y.id)
            }
        }
        val base = partitions.first()
        val rest = partitions.drop(1)
        val fnName = "endpoint_resolver"
        return RuntimeType.forInlineFun(fnName, RustModule.private("aws_endpoint")) {
            rustBlockTemplate("pub fn $fnName() -> impl #{ResolveAwsEndpoint}", *codegenScope) {
                withBlockTemplate("#{PartitionResolver}::new(", ")", *codegenScope) {
                    renderPartition(base)
                    rust(",")
                    withBlock("vec![", "]") {
                        rest.forEach {
                            renderPartition(it)
                            rust(",")
                        }
                    }
                }
            }
        }
    }

    private fun RustWriter.renderPartition(partition: PartitionNode) {
        /* Example:
        Partition::builder()
            .id("part-id-3")
            .region_regex(r#"^(eu)-\w+-\d+$"#)
            .default_endpoint(endpoint::Metadata {
                uri_template: "service.{region}.amazonaws.com",
                protocol: Https,
                signature_versions: &[V4],
                credential_scope: CredentialScope::builder()
                    .service("foo")
                    .build()
            })
            .endpoint(...)
            .build()
            .expect("valid partition")
         */
        rustTemplate(
            """
            #{Partition}::builder()
                .id(${partition.id.dq()})
                .region_regex(r##"${partition.regionRegex}"##)""",
            *codegenScope,
        )
        withBlock(".default_endpoint(", ")") {
            with(partition.defaults) {
                render()
            }
        }
        partition.partitionEndpoint?.also { ep ->
            rust(".partition_endpoint(${ep.dq()})")
        }
        when (partition.regionalized) {
            true -> rustTemplate(".regionalized(#{Regionalized}::Regionalized)", *codegenScope)
            false -> rustTemplate(".regionalized(#{Regionalized}::NotRegionalized)", *codegenScope)
        }
        partition.endpoints.forEach { (region, endpoint) ->
            withBlock(".endpoint(${region.dq()}, ", ")") {
                with(endpoint) {
                    render()
                }
            }
        }
        rust(""".build().expect("invalid partition")""")
    }

    inner class EndpointMeta(private val endpoint: ObjectNode, service: String, dnsSuffix: String) {
        private val uriTemplate =
            (endpoint.getStringMember("hostname").orNull() ?: throw CodegenException("endpoint must be defined"))
                .value
                .replace("{service}", service)
                .replace("{dnsSuffix}", dnsSuffix)
        private val credentialScope =
            CredentialScope(endpoint.getObjectMember("credentialScope").orElse(Node.objectNode()))

        private fun protocol(): String {
            val protocols = endpoint.expectArrayMember("protocols").map { it.expectStringNode().value }
            return if (protocols.contains("https")) {
                "Https"
            } else if (protocols.contains("http")) {
                "Http"
            } else {
                throw CodegenException("No protocol supported")
            }
        }

        private fun signatureVersion(): String {
            val signatureVersions = endpoint.expectArrayMember("signatureVersions").map { it.expectStringNode().value }
            // TODO(https://github.com/awslabs/smithy-rs/issues/977): we can use this to change the signing options instead of customizing S3 specifically
            if (!(signatureVersions.contains("v4") || signatureVersions.contains("s3v4"))) {
                throw CodegenException("endpoint does not support sigv4, unsupported: $signatureVersions")
            }
            return "V4"
        }

        fun RustWriter.render() {
            rustBlockTemplate("#{endpoint}::Metadata", *codegenScope) {
                rust("uri_template: ${uriTemplate.dq()},")
                rustTemplate("protocol: #{Protocol}::${protocol()},", *codegenScope)
                rustTemplate("signature_versions: #{SignatureVersion}::${signatureVersion()},", *codegenScope)
                withBlock("credential_scope: ", ",") {
                    with(credentialScope) {
                        render()
                    }
                }
            }
        }
    }

    /**
     * Represents a partition from endpoints.json
     */
    private inner class PartitionNode(endpointPrefix: String, config: ObjectNode) {
        // the partition id/name (e.g. "aws")
        val id: String = config.expectStringMember("partition").value

        // the node associated with [endpointPrefix] (or empty node)
        val service: ObjectNode = config
            .getObjectMember("services").orElse(Node.objectNode())
            .getObjectMember(endpointPrefix).orElse(Node.objectNode())

        // endpoints belonging to the service with the given [endpointPrefix] (or empty node)

        val dnsSuffix: String = config.expectStringMember("dnsSuffix").value

        // service specific defaults
        val defaults: EndpointMeta

        val endpoints: List<Pair<String, EndpointMeta>>

        init {

            val partitionDefaults = config.expectObjectMember("defaults")
            val serviceDefaults = service.getObjectMember("defaults").orElse(Node.objectNode())
            val mergedDefaults = partitionDefaults.merge(serviceDefaults)
            endpoints = service.getObjectMember("endpoints").orElse(Node.objectNode()).members.mapNotNull { (k, v) ->
                val endpointObject = mergedDefaults.merge(v.expectObjectNode())
                // There is no point in generating lots of endpoints that are just empty
                if (endpointObject != mergedDefaults) {
                    k.value to EndpointMeta(endpointObject, endpointPrefix, dnsSuffix)
                } else {
                    null
                }
            }

            defaults = EndpointMeta(mergedDefaults, endpointPrefix, dnsSuffix)
        }

        val regionalized: Boolean = service.getBooleanMemberOrDefault("isRegionalized", true)

        // regionalized services always use regionalized endpoints
        val partitionEndpoint: String? = if (regionalized) {
            null
        } else {
            service.getStringMember("partitionEndpoint").map(StringNode::getValue).orNull()
        }

        val regionRegex: String = config.expectStringMember("regionRegex").value
    }

    inner class CredentialScope(private val objectNode: ObjectNode) {
        fun RustWriter.render() {
            rustTemplate(
                """
                #{CredentialScope}::builder()
                """,
                *codegenScope,
            )
            objectNode.getStringMember("service").map {
                rustTemplate(
                    ".service(${it.value.dq()})",
                    *codegenScope,
                )
            }
            objectNode.getStringMember("region").map {
                rustTemplate(
                    ".region(${it.value.dq()})",
                    *codegenScope,
                )
            }
            rust(".build()")
        }
    }
}
