/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoints

import software.amazon.smithy.model.node.BooleanNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeType
import software.amazon.smithy.rulesengine.language.EndpointRuleSet
import software.amazon.smithy.rulesengine.language.syntax.parameters.Builtins
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameters
import software.amazon.smithy.rulesengine.traits.ContextIndex
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asType
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.raw
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustInline
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.toRustName

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

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations + CreateEndpointParams(
            codegenContext,
            operation,
            codegenContext.rootDecorator.builtInResolvers(codegenContext),
        )
    }

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        val endpointsIndex = EndpointsIndex.of(codegenContext.model)
        val endpointLib = RuntimeType.endpointLib(codegenContext.runtimeConfig)

        rustCrate.withNonRootModule(EndpointsModule.member("partition_metadata").fullyQualifiedName()) {
            generatePartitionMetadata(it)
        }

        endpointsIndex.rulesetForService(codegenContext.serviceShape)?.let { endpointRuleSet ->
            val endpointRuleSetVisitor = EndpointRuleSetVisitor(codegenContext.runtimeConfig, endpointRuleSet)
            val endpointParamsGenerator = EndpointParamsGenerator(endpointRuleSet.parameters.toList())
            rustCrate.withModule(EndpointsModule) { writer ->
                writer.rustTemplate(
                    """
                    mod partition_metadata;

                    pub type EndpointResult = Result<#{Endpoint}, #{Error}>;

                    pub(crate) fn resolve_endpoint(
                        params: #{Params},
                    ) -> EndpointResult {
                        #{Helpers:W}
                        #{DestructuredParams:W}

                        #{Rules:W}

                        // TODO How can we include the parameters in this error message?
                        ##[allow(unreachable_code)]
                        return Err(crate::endpoints::Error::endpoint_resolution("No rules matched.".into()));
                    }
                    """,
                    "Endpoint" to CargoDependency.SmithyHttp(codegenContext.runtimeConfig).asType()
                        .member("endpoint::Endpoint"),
                    "Error" to endpointParamsGenerator.error,
                    "Params" to endpointParamsGenerator.params,
                    "Helpers" to helpers(endpointLib),
                    "DestructuredParams" to destructuredParams(endpointRuleSet),
                    "Rules" to endpointRuleSetVisitor.visitRuleset(),
                )
            }

            endpointsIndex.testCasesForService(codegenContext.serviceShape)?.let { testCases ->
                EndpointTestsGenerator(codegenContext, endpointRuleSet, testCases).render(rustCrate)
            }
        }
    }

    private fun generatePartitionMetadata(writer: RustWriter) {
        writer.docs(
            """
            The default endpoint partition metadata for all supported regions. To use, deserialize it into
            a [`PartitionResolver`](crate::endpoint_lib::partition::PartitionResolver) by passing it to
            [`deserialize_partitions`](crate::endpoint_lib::partition::deser::deserialize_partitions).
            """.trimIndent(),
        )
        val partitionsJson =
            this::class.java.getResource("/software/amazon/smithy/rulesengine/language/partitions.json").readText()
        writer.raw("pub(crate) const PARTITION_METADATA: &str = ${partitionsJson.dq()};")
    }

    private fun helpers(endpointLib: RuntimeType) = writable {
        rustTemplate(
            """
                let mut diagnostic_collector = #{DiagnosticCollector}::new();
                let partition_resolver = #{deserialize_partitions}(#{PARTITION_METADATA}.as_bytes()).expect("partition metadata is valid");
            """,
            "DiagnosticCollector" to endpointLib.member("diagnostic::DiagnosticCollector"),
            "PartitionResolver" to endpointLib.member("partition::PartitionResolver"),
            "PARTITION_METADATA" to EndpointsModule.member("partition_metadata::PARTITION_METADATA"),
            "deserialize_partitions" to endpointLib.member("partition::deser::deserialize_partitions"),
        )
    }

    private fun destructuredParams(endpointRuleSet: EndpointRuleSet) = writable {
        endpointRuleSet.parameters.toList().forEach {
            rustInline("let ${it.name.toRustName()} = params.${it.name.toRustName()}")
            if (it.type.toString() == "String") {
                if (it.isRequired) {
                    rustInline(".as_str();")
                } else {
                    rustInline(".as_ref();")
                }
            } else {
                rustInline(";")
            }
        }
    }
}

val EndpointsModule = RustModule.public("endpoints", "Endpoints related functionality and tests")

/**
 * Creates an `<crate>::endpoint_resolver::Params` structure in make operation generator. This combines state from the
 * client, the operation, and the model to create parameters.
 *
 * Example generated code:
 * ```rust
 * let _endpoint_params = crate::endpoint_resolver::Params::builder()
 *     .set_region(Some("test-region"))
 *     .set_disable_everything(Some(true))
 *     .set_bucket(input.bucket.as_ref())
 *     .build();
 * ```
 */
class CreateEndpointParams(
    private val codegenContext: CodegenContext,
    private val operationShape: OperationShape,
    private val rulesEngineBuiltInResolvers: List<RulesEngineBuiltInResolver>,
) :
    OperationCustomization() {

    private val serviceShape = codegenContext.serviceShape
    private val model = codegenContext.model

    private val parameters =
        (
            EndpointsIndex.of(model).rulesetForService(serviceShape)?.parameters
                ?: Parameters.builder().addParameter(Builtins.REGION).build()
            ).toList()
    private val idx = ContextIndex.of(model)

    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateInput -> writable {
                val endpointParamsGenerator = EndpointParamsGenerator(parameters)

                rustTemplate(
                    "let endpoint_params = #{Params}::builder()#{builderFields:W}.build();",
                    "builderFields" to builderFields(section),
                    "Params" to endpointParamsGenerator.params,
                )
            }

            else -> emptySection
        }
    }

    private fun builderFields(section: OperationSection.MutateInput) = writable {
        fun setterName(str: String) = "set_${str.toRustName()}"
        val memberParams = idx.getContextParams(operationShape)
        val builtInParams = parameters.filter { it.isBuiltIn }
        // first load builtins and their defaults
        builtInParams.forEach { param ->
            val defaultProviders = rulesEngineBuiltInResolvers.mapNotNull { it.defaultFor(param, section.config) }
            if (defaultProviders.size > 1) {
                error("Multiple providers provided a value for the builtin $param")
            }
            defaultProviders.firstOrNull()?.also { defaultValue ->
                rust(".set_${param.name.toRustName()}(#W)", defaultValue)
            }
        }
        // NOTE(rcoh): we are not currently generating client context params onto the service shape yet
        // these can be overridden with client context params
        idx.getClientContextParams(serviceShape).orNull()?.parameters?.forEach { (name, param) ->
            val paramName = name.toRustName()
            val setterName = setterName(name)
            if (param.type == ShapeType.BOOLEAN) {
                rust(".$setterName(${section.config}.$paramName)")
            } else {
                rust(".$setterName(${section.config}.$paramName.as_ref())")
            }
        }

        idx.getStaticContextParams(operationShape).orNull()?.parameters?.forEach { (name, param) ->
            val setterName = setterName(name)
            val value = writable {
                when (val v = param.value) {
                    is BooleanNode -> rust("Some(${v.value})")
                    is StringNode -> rust("Some(${v.value})")
                    else -> TODO("Unexpected static value type: $v")
                }
            }
            rust(".$setterName(#W)", value)
        }

        // lastly, allow these to be overridden by members
        memberParams.forEach { (memberShape, param) ->
            rust(
                ".${setterName(param.name)}(${section.input}.${
                codegenContext.symbolProvider.toMemberName(
                    memberShape,
                )
                }.as_ref())",
            )
        }
    }
}
