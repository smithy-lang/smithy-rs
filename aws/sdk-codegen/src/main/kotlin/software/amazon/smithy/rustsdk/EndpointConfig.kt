/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.util.dq

class EndpointConfigCustomization(private val protocolConfig: ProtocolConfig) : ConfigCustomization() {
    private val endpointProvider = EndpointProvider(protocolConfig.runtimeConfig)
    private val endpointPrefix = protocolConfig.serviceShape.expectTrait(ServiceTrait::class.java).endpointPrefix
    override fun section(section: ServiceConfig): Writable = writable {
        when (section) {
            is ServiceConfig.ConfigStruct -> rust("pub endpoint_provider: ::std::sync::Arc<dyn #T>,", endpointProvider)
            is ServiceConfig.ConfigImpl -> emptySection
            is ServiceConfig.BuilderStruct ->
                rust("endpoint_provider: Option<::std::sync::Arc<dyn #T>>,", endpointProvider)
            ServiceConfig.BuilderImpl ->
                rust(
                    """
            pub fn endpoint_provider(mut self, endpoint_provider: impl #T + 'static) -> Self {
                self.endpoint_provider = Some(::std::sync::Arc::new(endpoint_provider));
                self
            }
            """,
                    endpointProvider
                )
            ServiceConfig.BuilderBuild -> rust(
                """endpoint_provider: self.endpoint_provider.unwrap_or_else(||
                                ::std::sync::Arc::new(
                                    #T::from_service_region(${endpointPrefix.dq()}, &region)
                                )
                         ),""",
                StaticEndpoint(protocolConfig.runtimeConfig)
            )
        }
    }
}

fun EndpointProvider(runtimeConfig: RuntimeConfig) = RuntimeType("ProvideEndpoint", CargoDependency.Operation(runtimeConfig), "operation::endpoint")
fun StaticEndpoint(runtimeConfig: RuntimeConfig) = RuntimeType("StaticEndpoint", CargoDependency.Operation(runtimeConfig), "operation::endpoint")

class EndpointConfigPlugin(private val operationShape: OperationShape) : OperationCustomization() {
    override fun section(section: OperationSection): Writable {
        if (operationShape.hasTrait(EndpointTrait::class.java)) {
            TODO()
        }
        return when (section) {
            OperationSection.ImplBlock -> emptySection
            OperationSection.Plugin -> writable {
                rust(
                    """
                use operation::endpoint::EndpointProviderExt;
                request.config.insert_endpoint_provider(_config.endpoint_provider.clone());
                """
                )
            }
        }
    }
}
