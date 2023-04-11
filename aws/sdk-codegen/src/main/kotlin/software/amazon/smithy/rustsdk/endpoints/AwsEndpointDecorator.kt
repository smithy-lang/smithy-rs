/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.endpoints

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.language.syntax.parameters.Builtins
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameters
import software.amazon.smithy.rulesengine.traits.EndpointRuleSetTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointsModule
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.util.extendIf
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.thenSingletonListOf
import software.amazon.smithy.rustsdk.AwsRuntimeType
import software.amazon.smithy.rustsdk.SdkConfigSection
import software.amazon.smithy.rustsdk.getBuiltIn

class AwsEndpointDecorator : ClientCodegenDecorator {
    override val name: String = "AwsEndpoint"
    override val order: Byte = 100

    override fun transformModel(service: ServiceShape, model: Model): Model {
        val customServices = setOf(
            ShapeId.from("com.amazonaws.s3#AmazonS3"),
            ShapeId.from("com.amazonaws.s3control#AWSS3ControlServiceV20180820"),
            ShapeId.from("com.amazonaws.codecatalyst#CodeCatalyst"),
        )
        if (customServices.contains(service.id)) {
            return model
        }
        // currently, most models incorrectly model region is optional when it is actually required—fix these models:
        return ModelTransformer.create().mapTraits(model) { _, trait ->
            when (trait) {
                is EndpointRuleSetTrait -> {
                    val epRules = EndpointRuleSet.fromNode(trait.ruleSet)
                    val newParameters = Parameters.builder()
                    epRules.parameters.toList()
                        .map { param ->
                            param.letIf(param.builtIn == Builtins.REGION.builtIn) { parameter ->
                                val builder = parameter.toBuilder().required(true)
                                // TODO(https://github.com/awslabs/smithy-rs/issues/2187): undo this workaround
                                parameter.defaultValue.ifPresent { default -> builder.defaultValue(default) }

                                builder.build()
                            }
                        }
                        .forEach(newParameters::addParameter)

                    val newTrait = epRules.toBuilder().parameters(
                        newParameters.build(),
                    ).build()
                    EndpointRuleSetTrait.builder().ruleSet(newTrait.toNode()).build()
                }

                else -> trait
            }
        }
    }

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations.extendIf(codegenContext.isRegionalized()) {
            AwsEndpointShimCustomization(codegenContext)
        }
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> {
        return baseCustomizations + PubUseEndpoint(codegenContext.runtimeConfig)
    }

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
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
        rustCrate.withModule(EndpointsModule) {
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

    class SdkEndpointCustomization(
        codegenContext: CodegenContext,
    ) :
        ConfigCustomization() {
        private val runtimeConfig = codegenContext.runtimeConfig
        private val resolveAwsEndpoint = AwsRuntimeType.awsEndpoint(runtimeConfig).resolve("ResolveAwsEndpoint")
        private val endpointShim = AwsRuntimeType.awsEndpoint(runtimeConfig).resolve("EndpointShim")
        private val codegenScope = arrayOf(
            "ResolveAwsEndpoint" to resolveAwsEndpoint,
            "EndpointShim" to endpointShim,
            "aws_types" to AwsRuntimeType.awsTypes(runtimeConfig),
        )

        override fun section(section: ServiceConfig): Writable = writable {
            when (section) {
                ServiceConfig.BuilderImpl -> rustTemplate(
                    """
                    /// Sets the endpoint url used to communicate with this service
                    ///
                    /// Note: this is used in combination with other endpoint rules, e.g. an API that applies a host-label prefix
                    /// will be prefixed onto this URL. To fully override the endpoint resolver, use
                    /// [`Builder::endpoint_resolver`].
                    pub fn endpoint_url(mut self, endpoint_url: impl Into<String>) -> Self {
                        self.endpoint_url = Some(endpoint_url.into());
                        self
                    }

                    /// Sets the endpoint url used to communicate with this service
                    ///
                    /// Note: this is used in combination with other endpoint rules, e.g. an API that applies a host-label prefix
                    /// will be prefixed onto this URL. To fully override the endpoint resolver, use
                    /// [`Builder::endpoint_resolver`].
                    pub fn set_endpoint_url(&mut self, endpoint_url: Option<String>) -> &mut Self {
                        self.endpoint_url = endpoint_url;
                        self
                    }
                    """,
                    *codegenScope,
                )

                ServiceConfig.BuilderBuild -> rust("endpoint_url: self.endpoint_url,")
                ServiceConfig.BuilderStruct -> rust("endpoint_url: Option<String>,")
                ServiceConfig.ConfigImpl -> {
                    Attribute.AllowDeadCode.render(this)
                    rust("pub(crate) fn endpoint_url(&self) -> Option<&str> { self.endpoint_url.as_deref() }")
                }

                ServiceConfig.ConfigStruct -> rust("endpoint_url: Option<String>,")
                ServiceConfig.ConfigStructAdditionalDocs -> emptySection
                ServiceConfig.Extras -> emptySection
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
}

fun ClientCodegenContext.isRegionalized() = getBuiltIn(Builtins.REGION) != null
