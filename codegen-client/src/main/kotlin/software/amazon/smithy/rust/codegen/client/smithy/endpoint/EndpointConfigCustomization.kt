/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asType
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable

class EndpointConfigCustomization(
    private val codegenContext: ClientCodegenContext,
    endpointCustomization: List<EndpointCustomization>,
) :
    ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val endpointsIndex = EndpointRulesetIndex.of(codegenContext.model)
    private val smithyEndpointResolver =
        CargoDependency.SmithyHttp(runtimeConfig).asType().member("endpoint::ResolveEndpoint")
    private val moduleUseName = codegenContext.moduleUseName()
    private val ruleset = endpointsIndex.endpointRulesForService(codegenContext.serviceShape)!!

    private val codegenScope = arrayOf(
        "SmithyResolver" to smithyEndpointResolver,
        "Params" to EndpointParamsGenerator(ruleset.parameters).paramsStruct(),
        "DefaultResolver" to EndpointResolverGenerator(
            endpointCustomization.flatMap {
                it.customRuntimeFunctions(
                    codegenContext,
                )
            },
            runtimeConfig,
        ).generateResolverStruct(ruleset),
    )

    override fun section(section: ServiceConfig): Writable = writable {
        val resolverTrait = "#{SmithyResolver}<#{Params}>"
        when (section) {
            is ServiceConfig.ConfigStruct -> rustTemplate(
                "pub (crate) endpoint_resolver: std::sync::Arc<dyn $resolverTrait>,",
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
                rustTemplate(
                    "endpoint_resolver: Option<std::sync::Arc<dyn $resolverTrait>>,",
                    *codegenScope,
                )

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
                    /// // TODO: example
                    /// ```
                    pub fn endpoint_resolver(mut self, endpoint_resolver: impl $resolverTrait + 'static) -> Self {
                        self.endpoint_resolver = Some(std::sync::Arc::new(endpoint_resolver) as _);
                        self
                    }

                    /// Sets the endpoint resolver to use when making requests.
                    pub fn set_endpoint_resolver(&mut self, endpoint_resolver: Option<std::sync::Arc<dyn $resolverTrait>>) -> &mut Self {
                        self.endpoint_resolver = endpoint_resolver;
                        self
                    }
                    """,
                    *codegenScope,
                )

            ServiceConfig.BuilderBuild -> {
                rustTemplate(
                    """
                    endpoint_resolver: self.endpoint_resolver.unwrap_or_else(||
                        std::sync::Arc::new(#{DefaultResolver}::new())
                    ),
                    """,
                    *codegenScope,
                )
            }

            else -> emptySection
        }
    }
}
