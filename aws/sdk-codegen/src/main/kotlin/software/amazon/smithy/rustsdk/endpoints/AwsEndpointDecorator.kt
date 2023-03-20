/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.endpoints

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.rulesengine.language.syntax.parameters.Builtins
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.client.smithy.featureGatedConfigModule
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rust.codegen.core.util.extendIf
import software.amazon.smithy.rust.codegen.core.util.thenSingletonListOf
import software.amazon.smithy.rustsdk.AwsRuntimeType
import software.amazon.smithy.rustsdk.SdkConfigSection
import software.amazon.smithy.rustsdk.getBuiltIn

class AwsEndpointDecorator : ClientCodegenDecorator {
    override val name: String = "AwsEndpoint"
    override val order: Byte = 100

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations.extendIf(codegenContext.isRegionalized()) {
            AwsEndpointShimCustomization(codegenContext)
        }
    }

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        rustCrate.withModule(codegenContext.featureGatedConfigModule()) {
            rust(
                "pub use #T::endpoint::Endpoint;",
                CargoDependency.smithyHttp(codegenContext.runtimeConfig).toType(),
            )
        }

        val epTypes = EndpointTypesGenerator.fromContext(codegenContext)
        if (epTypes.defaultResolver() == null) {
            throw CodegenException(
                "${codegenContext.serviceShape} did not provide endpoint rules. " +
                    "This is a bug and the generated client will not work. All AWS services MUST define endpoint rules.",
            )
        }
        // generate a region converter if params has a region
        if (!codegenContext.isRegionalized()) {
            println("not generating a resolver for ${codegenContext.serviceShape}")
            return
        }
        rustCrate.withModule(ClientRustModule.Endpoint) {
            // TODO(https://github.com/awslabs/smithy-rs/issues/1784) cleanup task
            rustTemplate(
                """
                /// Temporary shim to allow new and old endpoint resolvers to co-exist
                ///
                /// This enables converting from the actual parameters type to the placeholder parameters type that
                /// contains a region
                ##[doc(hidden)]
                impl From<#{Params}> for #{PlaceholderParams} {
                    fn from(params: #{Params}) -> Self {
                        Self::new(params.region().map(|r|#{Region}::new(r.to_string())))
                    }
                }
                """,
                "Params" to epTypes.paramsStruct(),
                "Region" to AwsRuntimeType.awsTypes(codegenContext.runtimeConfig).resolve("region::Region"),
                "PlaceholderParams" to AwsRuntimeType.awsEndpoint(codegenContext.runtimeConfig).resolve("Params"),
            )
        }
    }

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
        return codegenContext.isRegionalized().thenSingletonListOf {
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rust(
                    """
                    ${section.serviceConfigBuilder}.set_aws_endpoint_resolver(${section.sdkConfig}.endpoint_resolver().clone());
                    """,
                )
            }
        }
    }

    class AwsEndpointShimCustomization(codegenContext: ClientCodegenContext) : ConfigCustomization() {
        private val moduleUseName = codegenContext.moduleUseName()
        private val runtimeConfig = codegenContext.runtimeConfig
        private val resolveAwsEndpoint = AwsRuntimeType.awsEndpoint(runtimeConfig).resolve("ResolveAwsEndpoint")
        private val endpointShim = AwsRuntimeType.awsEndpoint(runtimeConfig).resolve("EndpointShim")
        private val codegenScope = arrayOf(
            "ResolveAwsEndpoint" to resolveAwsEndpoint,
            "EndpointShim" to endpointShim,
            "aws_types" to AwsRuntimeType.awsTypes(runtimeConfig),
        )

        override fun section(section: ServiceConfig) = writable {
            when (section) {
                ServiceConfig.BuilderImpl -> rustTemplate(
                    """
                    /// Overrides the endpoint resolver to use when making requests.
                    ///
                    /// This method is deprecated, use [`Builder::endpoint_url`] or [`Builder::endpoint_resolver`] instead.
                    ///
                    /// When unset, the client will used a generated endpoint resolver based on the endpoint metadata
                    /// for `$moduleUseName`.
                    ///
                    /// ## Examples
                    /// ```no_run
                    /// ## fn wrapper() -> Result<(), aws_smithy_http::endpoint::error::InvalidEndpointError> {
                    /// use $moduleUseName::config::{Config, Endpoint, Region};
                    ///
                    /// let config = Config::builder()
                    ///     .endpoint_resolver(Endpoint::immutable("http://localhost:8080")?)
                    ///     .build();
                    /// ## Ok(())
                    /// ## }
                    /// ```
                    ##[deprecated(note = "use endpoint_url or set the endpoint resolver directly")]
                    pub fn aws_endpoint_resolver(mut self, endpoint_resolver: impl #{ResolveAwsEndpoint} + 'static) -> Self {
                        self.endpoint_resolver = Some(std::sync::Arc::new(#{EndpointShim}::from_resolver(endpoint_resolver)) as _);
                        self
                    }

                    ##[deprecated(note = "use endpoint_url or set the endpoint resolver directly")]
                    /// Sets the endpoint resolver to use when making requests.
                    ///
                    /// This method is deprecated, use [`Builder::endpoint_url`] or [`Builder::endpoint_resolver`] instead.
                    pub fn set_aws_endpoint_resolver(&mut self, endpoint_resolver: Option<std::sync::Arc<dyn #{ResolveAwsEndpoint}>>) -> &mut Self {
                        self.endpoint_resolver = endpoint_resolver.map(|res|std::sync::Arc::new(#{EndpointShim}::from_arc(res) ) as _);
                        self
                    }

                    """,
                    *codegenScope,
                )

                else -> emptySection
            }
        }
    }
}

fun ClientCodegenContext.isRegionalized() = getBuiltIn(Builtins.REGION) != null
