/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoints

import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rulesengine.traits.EndpointRuleSetTrait
import software.amazon.smithy.rulesengine.traits.EndpointTestsTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.raw
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.UNREACHABLE
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import java.nio.file.FileSystems
import java.nio.file.Files
import kotlin.io.path.extension
import kotlin.io.path.nameWithoutExtension
import kotlin.io.path.readText

/**
 * BuiltInResolver enables potentially external codegen stages to provide sources for `builtIn` parameters.
 * For example, allows AWS to provide the value for the region builtIn in separate codegen.
 *
 * If this resolver does not recognize the value, it MUST return `null`.
 */
interface RulesEngineBuiltInResolver {
    fun defaultFor(parameter: Parameter, configRef: String): Writable?
}

class EndpointsDecorator : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    override val name: String = "Endpoints"
    override val order: Byte = 0

    private fun applies(codegenContext: CodegenContext): Boolean =
        codegenContext.serviceShape.hasTrait<EndpointRuleSetTrait>()

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)

//    override fun operationCustomizations(
//        codegenContext: ClientCodegenContext,
//        operation: OperationShape,
//        baseCustomizations: List<OperationCustomization>,
//    ): List<OperationCustomization> {
//        return baseCustomizations + CreateEndpointParams(
//            codegenContext,
//            operation,
//            codegenContext.rootDecorator.builtInResolvers(codegenContext),
//        )
//    }

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        if (!applies(codegenContext)) {
            return
        }

        val endpointLib = inlineEndpointLibModule(codegenContext, rustCrate)

        val endpointRuleSet = codegenContext.serviceShape.expectTrait<EndpointRuleSetTrait>().let {
            EndpointRuleSet.fromNode(it.ruleSet)
        }

        val endpointParamsGenerator = EndpointParamsGenerator(endpointRuleSet)
        EndpointResolverGenerator(
            codegenContext,
            endpointRuleSet,
            endpointParamsGenerator.params,
            endpointParamsGenerator.error,
            endpointLib,
        ).generateResolver(rustCrate)

        codegenContext.serviceShape.getTrait<EndpointTestsTrait>()?.also { endpointTestsTrait ->
            EndpointTestsGenerator(codegenContext, endpointRuleSet, endpointTestsTrait.testCases).render(rustCrate)
        }
    }

    private fun inlineEndpointLibModule(codegenContext: ClientCodegenContext, rustCrate: RustCrate): InlineDependency {
        println("Setting up endpoint_lib modules for ${codegenContext.settings.moduleName}")

        val module = RustModule.default("endpoint_lib", visibility = Visibility.PUBCRATE)
        val additionalDependencies = listOf(
            CargoDependency.Http,
            CargoDependency.Url,
            CargoDependency.Regex,
            CargoDependency.smithyJson(codegenContext.runtimeConfig),
        )

        val rustFile = this::class.java.getResource("/inlineable/src/endpoint_lib.rs") ?: UNREACHABLE("known to exist")
        val rootModule = InlineDependency(name, module, additionalDependencies) { writer ->
            writer.raw(rustFile.readText())
        }

        this::class.java.getResource("/inlineable/src/endpoint_lib")?.let {
            FileSystems.newFileSystem(it.toURI(), emptyMap<String, String>()).use { fs ->
                Files.walk(fs.getPath("/inlineable/src/endpoint_lib"))
                    .filter { file -> file.extension == "rs" }
                    .forEach { file ->
                        println("adding module for ${file.fileName}")
                        rustCrate.withNonRootModule("crate::endpoint_lib::${file.nameWithoutExtension}") { moduleWriter ->
                            moduleWriter.raw(file.readText())
                        }
                    }
            }
        }

        return rootModule
    }
}

internal val EndpointsModule = RustModule.public("endpoints", "Endpoint parameter struct and builder")

///**
// * Creates an `<crate>::endpoint_resolver::Params` structure in make operation generator. This combines state from the
// * client, the operation, and the model to create parameters.
// *
// * Example generated code:
// * ```rust
// * let _endpoint_params = crate::endpoint_resolver::Params::builder()
// *     .set_region(Some("test-region"))
// *     .set_disable_everything(Some(true))
// *     .set_bucket(input.bucket.as_ref())
// *     .build();
// * ```
// */
//class CreateEndpointParams(
//    private val ctx: CodegenContext,
//    private val operationShape: OperationShape,
//    private val rulesEngineBuiltInResolvers: List<RulesEngineBuiltInResolver>,
//) :
//    OperationCustomization() {
//
//    private val runtimeConfig = ctx.runtimeConfig
//    private val params =
//        EndpointRuleSetIndex.of(ctx.model).endpointRulesForService(ctx.serviceShape)?.parameters
//            ?: Parameters.builder().addParameter(Builtins.REGION).build()
//    private val idx = ContextIndex.of(ctx.model)
//
//    private val codegenScope = arrayOf(
//        "Params" to EndpointParamsGenerator(params).paramsStruct(),
//        "BuildError" to runtimeConfig.operationBuildError(),
//    )
//
//    override fun section(section: OperationSection): Writable {
//        return when (section) {
//            is OperationSection.MutateInput -> writable {
//                // insert the endpoint resolution _result_ into the bag (note that this won't bail if endpoint
//                // resolution failed)
//                // generate with a leading `_` because we aren't generating rules that will use this for all services
//                // yet.
//                rustTemplate(
//                    """
//                    let _endpoint_params = #{Params}::builder()#{builderFields:W}.build();
//                    """,
//                    "builderFields" to builderFields(section),
//                    *codegenScope,
//                )
//            }
//
//            else -> emptySection
//        }
//    }
//
//    private fun builderFields(section: OperationSection.MutateInput) = writable {
//        val memberParams = idx.getContextParams(operationShape)
//        val builtInParams = params.toList().filter { it.isBuiltIn }
//        // first load builtins and their defaults
//        builtInParams.forEach { param ->
//            val defaultProviders = rulesEngineBuiltInResolvers.mapNotNull { it.defaultFor(param, section.config) }
//            if (defaultProviders.size > 1) {
//                error("Multiple providers provided a value for the builtin $param")
//            }
//            defaultProviders.firstOrNull()?.also { defaultValue ->
//                rust(".set_${param.name.rustName()}(#W)", defaultValue)
//            }
//        }
//        // NOTE(rcoh): we are not currently generating client context params onto the service shape yet
//        // these can be overridden with client context params
//        idx.getClientContextParams(ctx.serviceShape).orNull()?.parameters?.forEach { (name, param) ->
//            val paramName = EndpointParamsGenerator.memberName(name)
//            val setterName = EndpointParamsGenerator.setterName(name)
//            if (param.type == ShapeType.BOOLEAN) {
//                rust(".$setterName(${section.config}.$paramName)")
//            } else {
//                rust(".$setterName(${section.config}.$paramName.as_ref())")
//            }
//        }
//
//        idx.getStaticContextParams(operationShape).orNull()?.parameters?.forEach { (name, param) ->
//            val setterName = EndpointParamsGenerator.setterName(name)
//            val value = writable {
//                when (val v = param.value) {
//                    is BooleanNode -> rust("Some(${v.value})")
//                    is StringNode -> rust("Some(${v.value})")
//                    else -> TODO("Unexpected static value type: $v")
//                }
//            }
//            rust(".$setterName(#W)", value)
//        }
//
//        // lastly, allow these to be overridden by members
//        memberParams.forEach { (memberShape, param) ->
//            rust(
//                ".${EndpointParamsGenerator.setterName(param.name)}(${section.input}.${
//                    ctx.symbolProvider.toMemberName(
//                        memberShape,
//                    )
//                }.as_ref())",
//            )
//        }
//    }
//}

//class EndpointsGenerator(
//    private val codegenContext: ClientCodegenContext,
//    private val endpointRuleSetTrait: EndpointRuleSetTrait,
//) {
//    private val rules: EndpointRuleSet by lazy {
//        EndpointRuleSet.fromNode(endpointRuleSetTrait.ruleSet)
//    }
//    private val generator: EndpointsRulesGenerator by lazy { EndpointsRulesGenerator(rules, codegenContext.runtimeConfig) }
//
//    fun render(crate: RustCrate): RuntimeType {
//        // Render endpoint tests if test cases exist
//        codegenContext.serviceShape.getTrait<EndpointTestsTrait>()?.testCases?.let { testCases ->
//            // TODO(zelda) can I use John's `withNonRootModule` for this?
//            // TODO(zelda) it feels weird to be doing this as a side effect in a function that returns something
//            crate.lib {
//                it.withModule("tests", RustMetadata(additionalAttributes = listOf(Attribute.Cfg("test")))) {
//                    EndpointTestsGenerator(
//                        codegenContext,
//                        testCases,
//                        generator,
//                        rules
//                    ).generate()(this)
//                }
//            }
//        }
//
//        return generator.endpointResolver()
//    }
//}
