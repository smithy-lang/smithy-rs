/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Local
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.util.dq

class AwsEndpointDecorator : RustCodegenDecorator {
    override val name: String = "AwsEndpoint"
    override val order: Byte = 0

    override fun configCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations + EndpointConfigCustomization(
            protocolConfig.runtimeConfig,
            protocolConfig.serviceShape
        )
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

class EndpointConfigCustomization(private val runtimeConfig: RuntimeConfig, serviceShape: ServiceShape) :
    ConfigCustomization() {
    private val endpointPrefix = serviceShape.expectTrait(ServiceTrait::class.java).endpointPrefix
    private val resolveAwsEndpoint = runtimeConfig.awsEndpointDependency().asType().copy(name = "ResolveAwsEndpoint")
    override fun section(section: ServiceConfig): Writable = writable {
        when (section) {
            is ServiceConfig.ConfigStruct -> rust(
                "pub endpoint_resolver: ::std::sync::Arc<dyn #T>,",
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
            ServiceConfig.BuilderBuild -> rust(
                """endpoint_resolver: self.endpoint_resolver.unwrap_or_else(||
                                ::std::sync::Arc::new(
                                    #T::DefaultAwsEndpointResolver::for_service(${endpointPrefix.dq()})
                                )
                         ),""",
                runtimeConfig.awsEndpointDependency().asType()
            )
        }
    }
}

// This is an experiment in a slightly different way to create runtime types. All code MAY be refactored to use this pattern
fun RuntimeConfig.awsEndpointDependency() = CargoDependency("aws-endpoint", Local(this.relativePath))

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
        }
    }
}
