/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.CustomRuntimeFunction
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.endpointTestsModule
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.serviceSpecificEndpointResolver
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.SmithyEndpointsStdLib
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate

/**
 * BuiltInResolver enables potentially external codegen stages to provide sources for `builtIn` parameters.
 * For example, this allows AWS to provide the value for the region builtIn in separate codegen.
 *
 * If this resolver does not recognize the value, it MUST return `null`.
 */
interface EndpointCustomization {
    /**
     * Provide the default value for [parameter] given a reference to the service config struct ([configRef])
     *
     * If this parameter is not recognized, return null.
     *
     * Example:
     * ```kotlin
     * override fun loadBuiltInFromServiceConfig(parameter: Parameter, configRef: String): Writable? {
     *     return when (parameter.builtIn) {
     *         AwsBuiltIns.REGION.builtIn -> writable { rust("$configRef.region.as_ref().map(|r|r.as_ref().to_owned())") }
     *         else -> null
     *     }
     * }
     * ```
     */
    fun loadBuiltInFromServiceConfig(
        parameter: Parameter,
        configRef: String,
    ): Writable? = null

    /**
     * Set a given builtIn value on the service config builder. If this builtIn is not recognized, return null
     *
     * Example:
     * ```kotlin
     * override fun setBuiltInOnServiceConfig(name: String, value: Node, configBuilderRef: String): Writable? {
     *     if (name != AwsBuiltIns.REGION.builtIn.get()) {
     *         return null
     *     }
     *     return writable {
     *         rustTemplate(
     *             "let $configBuilderRef = $configBuilderRef.region(#{Region}::new(${value.expectStringNode().value.dq()}));",
     *             "Region" to region(codegenContext.runtimeConfig).resolve("Region"),
     *         )
     *     }
     * }
     * ```
     */

    fun setBuiltInOnServiceConfig(
        name: String,
        value: Node,
        configBuilderRef: String,
    ): Writable? = null

    /**
     * Provide a list of additional endpoints standard library functions that rules can use
     */
    fun customRuntimeFunctions(codegenContext: ClientCodegenContext): List<CustomRuntimeFunction> = listOf()

    /**
     * Allows overriding the `finalize_params` method  in the `ResolveEndpoint` trait within
     * service-specific EndpointResolver, which is rendered in `serviceSpecificEndpointResolver`.
     *
     * `ResolveEndpoint::finalize_params` provides a default no-op implementation,
     * and this customization enables the implementor to provide an alternative implementation.
     *
     * Example:
     * ```kotlin
     * override fun serviceSpecificEndpointParamsFinalizer(codegenContext: ClientCodegenContext, params: String): Writable? {
     *     return writable {
     *         rustTemplate("""
     *             let identity = $params
     *                 .get_property_mut::<#{Identity}>();
     *             // do more things on `params`
     *         """,
     *         "Identity" to RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig)
     *             .resolve("client::identity::Identity"),
     *         )
     *     }
     * }
     * ```
     */
    fun serviceSpecificEndpointParamsFinalizer(
        codegenContext: ClientCodegenContext,
        params: String,
    ): Writable? = null

    /**
     * Allows injecting validation logic for endpoint parameters into the `ParamsBuilder::build` method.
     *
     * e.g. when generating the builder for the endpoint parameters this allows you to insert validation logic before
     * being finalizing the parameters.
     *
     * ```rs
     * impl ParamsBuilder {
     *     pub fn build(self) -> ::std::result::Result<crate::config::endpoint::Params, crate::config::endpoint::InvalidParams> {
     *         <validation logic>
     *         ...
     *     }
     * }
     *
     * Example:
     * ```kotlin
     *
     *  override fun endpointParamsBuilderValidator(codegenContext: ClientCodegenContext, parameter: Parameter): Writable? {
     *  rustTemplate("""
     *      if let Some(region) = self.${parameter.memberName()} {
     *          if #{is_valid_host_label}(region.as_ref() as &str, false, #{DiagnosticCollector}::new()) {
     *              return Err(#{ParamsError}::invalid_value(${parameter.memberName().dq()}, "must be a valid host label"))
     *          }
     *      }
     *      """,
     *          "is_valid_host_label" to EndpointsLib.isValidHostLabel,
     *          "ParamsError" to EndpointParamsGenerator.paramsError(),
     *          "DiagnosticCollector" to EndpointsLib.DiagnosticCollector,
     *       )
     *   }
     * ```
     */
    fun endpointParamsBuilderValidator(
        codegenContext: ClientCodegenContext,
        parameter: Parameter,
    ): Writable? = null
}

/**
 * Decorator that injects endpoints 2.0 resolvers throughout the client.
 *
 * 1. Add ClientContext params to the config struct
 * 2. Inject params / endpoint results into the operation properties
 * 3. Set a default endpoint resolver (when available)
 * 4. Create an endpoint params structure/builder
 * 5. Generate endpoint tests (when available)
 *
 * This decorator installs the core standard library functions. It DOES NOT inject the AWS specific functions which
 * must be injected separately.
 *
 * If the service DOES NOT provide custom endpoint rules, this decorator is a no-op.
 */
class EndpointsDecorator : ClientCodegenDecorator {
    override val name: String = "Endpoints"
    override val order: Byte = 0

    override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> {
        return listOf(
            object : EndpointCustomization {
                override fun customRuntimeFunctions(codegenContext: ClientCodegenContext): List<CustomRuntimeFunction> {
                    return SmithyEndpointsStdLib
                }
            },
        )
    }

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations + ClientContextConfigCustomization(codegenContext) +
            EndpointConfigCustomization(codegenContext, EndpointTypesGenerator.fromContext(codegenContext))
    }

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> {
        return baseCustomizations +
            object : ServiceRuntimePluginCustomization() {
                override fun section(section: ServiceRuntimePluginSection): Writable {
                    return when (section) {
                        is ServiceRuntimePluginSection.RegisterRuntimeComponents ->
                            writable {
                                codegenContext.defaultEndpointResolver()?.also { resolver ->
                                    section.registerEndpointResolver(this, resolver)
                                }
                            }

                        else -> emptySection
                    }
                }
            }
    }

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        val generator = EndpointTypesGenerator.fromContext(codegenContext)
        rustCrate.withModule(ClientRustModule.Config.endpoint) {
            withInlineModule(endpointTestsModule(), rustCrate.moduleDocProvider) {
                generator.testGenerator()(this)
            }
        }
        rustCrate.mergeFeature(
            Feature(
                "gated-tests",
                default = false,
                emptyList(),
            ),
        )
    }
}

/**
 * Returns the rules-generated endpoint resolver for this service
 *
 * If no endpoint rules are provided, `null` will be returned.
 */
private fun ClientCodegenContext.defaultEndpointResolver(): Writable? {
    val generator = EndpointTypesGenerator.fromContext(this)
    val defaultResolver = generator.defaultResolver() ?: return null
    val ctx =
        arrayOf("DefaultResolver" to defaultResolver, "ServiceSpecificResolver" to serviceSpecificEndpointResolver())
    return writable {
        rustTemplate(
            """{
            use #{ServiceSpecificResolver};
            #{DefaultResolver}::new().into_shared_resolver()
            }""",
            *ctx,
        )
    }
}
