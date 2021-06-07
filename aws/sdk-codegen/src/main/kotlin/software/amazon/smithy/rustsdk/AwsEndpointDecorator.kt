/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.orNull

class AwsEndpointDecorator : RustCodegenDecorator {
    override val name: String = "AwsEndpoint"
    override val order: Byte = 0

    private val endpoints by lazy {
        val endpointsJson = javaClass.getResource("endpoints.json")!!.readText()
        Node.parse(endpointsJson).expectObjectNode()
    }

    override fun configCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations + EndpointConfigCustomization(protocolConfig, endpoints)
    }

    override fun operationCustomizations(
        protocolConfig: ProtocolConfig,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        return baseCustomizations + EndpointResolverFeature(protocolConfig.runtimeConfig, operation)
    }

    override fun libRsCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        return baseCustomizations + PubUseEndpoint(protocolConfig.runtimeConfig)
    }
}

class EndpointConfigCustomization(private val protocolConfig: ProtocolConfig, private val endpointData: ObjectNode) :
    ConfigCustomization() {
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val resolveAwsEndpoint = runtimeConfig.awsEndpointDependency().asType().copy(name = "ResolveAwsEndpoint")
    override fun section(section: ServiceConfig): Writable = writable {
        when (section) {
            is ServiceConfig.ConfigStruct -> rust(
                "pub (crate) endpoint_resolver: ::std::sync::Arc<dyn #T>,",
                resolveAwsEndpoint
            )
            is ServiceConfig.ConfigImpl -> emptySection
            is ServiceConfig.BuilderStruct ->
                rust("endpoint_resolver: Option<::std::sync::Arc<dyn #T>>,", resolveAwsEndpoint)
            ServiceConfig.BuilderImpl ->
                rust(
                    """
            pub fn endpoint_resolver(mut self, endpoint_resolver: impl #T + 'static) -> Self {
                self.endpoint_resolver = Some(::std::sync::Arc::new(endpoint_resolver));
                self
            }
            """,
                    resolveAwsEndpoint
                )
            ServiceConfig.BuilderBuild -> {
                val resolverGenerator = EndpointResolverGenerator(protocolConfig, endpointData)
                rust(
                    """endpoint_resolver: self.endpoint_resolver.unwrap_or_else(||
                                ::std::sync::Arc::new(
                                    #T()
                                )
                         ),""",
                    resolverGenerator.resolver(),
                )
            }
        }
    }
}

// This is an experiment in a slightly different way to create runtime types. All code MAY be refactored to use this pattern
fun RuntimeConfig.awsEndpointDependency() = awsRuntimeDependency("aws-endpoint")

class EndpointResolverFeature(private val runtimeConfig: RuntimeConfig, private val operationShape: OperationShape) :
    OperationCustomization() {
    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateRequest -> writable {
                rust(
                    """
                #T::set_endpoint_resolver(&mut ${section.request}.config_mut(), ${section.config}.endpoint_resolver.clone());
                """,
                    runtimeConfig.awsEndpointDependency().asType()
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
                    CargoDependency.SmithyHttp(runtimeConfig).asType()
                )
            }
            else -> emptySection
        }
    }
}

class EndpointResolverGenerator(protocolConfig: ProtocolConfig, private val endpointData: ObjectNode) {
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val endpointPrefix = protocolConfig.serviceShape.expectTrait<ServiceTrait>().endpointPrefix
    private val awsEndpoint = runtimeConfig.awsEndpointDependency().asType()
    private val codegenScope =
        arrayOf(
            "Partition" to awsEndpoint.member("Partition"),
            "CompleteEndpoint" to awsEndpoint.member("partition::endpoint::CompleteEndpoint"),
            "PartialEndpoint" to awsEndpoint.member("partition::endpoint::PartialEndpoint"),
            "CredentialScope" to awsEndpoint.member("CredentialScope"),
            "Regionalized" to awsEndpoint.member("partition::Regionalized"),
            "Protocol" to awsEndpoint.member("partition::endpoint::Protocol"),
            "SignatureVersion" to awsEndpoint.member("partition::endpoint::SignatureVersion")
        )

    fun resolver(): RuntimeType {
        val partitionsData = endpointData.expectArrayMember("partitions").getElementsAs(Node::expectObjectNode)

        val comparePartitions = object : Comparator<PartitionNode> {
            override fun compare(x: PartitionNode, y: PartitionNode): Int {
                // always sort standard aws partition first
                if (x.id == "aws") return -1
                return x.id.compareTo(y.id)
            }
        }

        val partitions = partitionsData.map {
            PartitionNode(endpointPrefix, it)
        }.sortedWith(comparePartitions)
        val base = partitions.first()
        val rest = partitions.drop(1)
        val fnName = "endpoint_resolver"
        return RuntimeType.forInlineFun(fnName, "aws_endpoint") {
            it.rustBlock("pub fn $fnName() -> impl #T", awsEndpoint.member("ResolveAwsEndpoint")) {
                withBlock("#T::new(", ")", awsEndpoint.member("PartitionResolver")) {
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
        rustTemplate(
            """
            #{Partition}::builder()
                .id(${partition.id.dq()})
                .region_regex(r##"${partition.regionRegex}"##)""",
            *codegenScope
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

        /*
        Partition::builder()
            .id("part-id-3")
            .region_regex(r#"^(eu)-\w+-\d+$"#)
            .default_endpoint(CompleteEndpoint {
                uri_template: "service.{region}.amazonaws.com",
                protocol: Https,
                signature_versions: &[V4],
                credential_scope: CredentialScope {
                    service: Some(SigningService::from_static("foo")),
                    ..Default::default()
                },
            })
            .build()
            .expect("valid partition")
         */
    }

    inner class EndpointMeta(private val endpoint: ObjectNode, service: String, dnsSuffix: String) {
        val uriTemplate =
            (endpoint.getStringMember("hostname").orNull() ?: throw CodegenException("endpoint must be defined"))
                .value
                .replace("{service}", service)
                .replace("{dnsSuffix}", dnsSuffix)
        val credentialScope = CredentialScope(endpoint.getObjectMember("credentialScope").orElse(Node.objectNode()))
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
            // TODO: we can use this to change the signing options
            if (!(signatureVersions.contains("v4") || signatureVersions.contains("s3v4"))) {
                throw CodegenException("endpoint does not support sigv4, unsupported: $signatureVersions")
            }
            return "V4"
        }

        fun RustWriter.render() {
            rustBlockTemplate("#{CompleteEndpoint}", *codegenScope) {
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
    private inner class PartitionNode(endpointPrefix: String, val config: ObjectNode) {
        // the partition id/name (e.g. "aws")
        val id = config.expectStringMember("partition").value

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
            endpoints = service.getObjectMember("endpoints").orElse(Node.objectNode()).members.map { (k, v) ->
                val endpointObject = mergedDefaults.merge(v.expectObjectNode())
                k.value to EndpointMeta(endpointObject, endpointPrefix, dnsSuffix)
            }

            defaults = EndpointMeta(mergedDefaults, endpointPrefix, dnsSuffix)
        }

        val regionalized = service.getBooleanMemberOrDefault("isRegionalized", true)

        // regionalized services always use regionalized endpoints
        val partitionEndpoint: String? = if (regionalized) {
            null
        } else {
            service.getStringMember("partitionEndpoint").map(StringNode::getValue).orNull()
        }

        val regionRegex = config.expectStringMember("regionRegex").value
    }

    inner class CredentialScope(private val objectNode: ObjectNode) {
        fun RustWriter.render() {
            rustTemplate(
                """
                #{CredentialScope}::builder()
            """,
                *codegenScope
            )
            objectNode.getStringMember("service").map {
                rust(".service(${it.value.dq()})")
            }
            objectNode.getStringMember("region").map {
                rust(".region(${it.value.dq()})")
            }
            rust(".build()")
        }
    }
}
