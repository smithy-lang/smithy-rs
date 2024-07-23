/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.smoketests.model.AwsSmokeTestModel
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.cfg
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.smoketests.traits.Expectation
import software.amazon.smithy.smoketests.traits.SmokeTestCase
import software.amazon.smithy.smoketests.traits.SmokeTestsTrait
import java.util.Optional

class SmokeTestsDecorator : ClientCodegenDecorator {
    override val name: String = "SmokeTests"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations.letIf(operation.hasTrait(SmokeTestsTrait.ID)) {
            it + SmokeTestsOperationCustomization(codegenContext, operation)
        }
}

class SmokeTestsOperationCustomization(
    private val codegenContext: ClientCodegenContext,
    private val operation: OperationShape,
) : OperationCustomization() {
    private val smokeTestsTrait = operation.expectTrait<SmokeTestsTrait>()
    private val testCases: List<SmokeTestCase> = smokeTestsTrait.testCases
    private val operationInput = operation.inputShape(codegenContext.model)

    override fun section(section: OperationSection): Writable {
        if (testCases.isEmpty()) return emptySection

        return when (section) {
            is OperationSection.UnitTests ->
                writable {
                    testCases.forEach { testCase ->
                        Attribute(cfg("v2SmokeTests")).render(this)
                        Attribute.TokioTest.render(this)
                        this.rustBlock("async fn test_${testCase.id.toSnakeCase()}()") {
                            val instantiator = SmokeTestsInstantiator(codegenContext)
                            instantiator.renderConf(this, testCase)
                            rust("let client = crate::Client::from_conf(conf);")
                            instantiator.renderInput(this, operation, operationInput, testCase.params)
                            instantiator.renderExpectation(this, testCase.expectation)
                        }
                    }
                }

            else -> emptySection
        }
    }
}

class SmokeTestsBuilderKindBehavior(val codegenContext: CodegenContext) : Instantiator.BuilderKindBehavior {
    override fun hasFallibleBuilder(shape: StructureShape): Boolean =
        BuilderGenerator.hasFallibleBuilder(shape, codegenContext.symbolProvider)

    override fun setterName(memberShape: MemberShape): String = memberShape.setterName()

    override fun doesSetterTakeInOption(memberShape: MemberShape): Boolean = true
}

class SmokeTestsInstantiator(private val codegenContext: ClientCodegenContext) : Instantiator(
    codegenContext.symbolProvider,
    codegenContext.model,
    codegenContext.runtimeConfig,
    SmokeTestsBuilderKindBehavior(codegenContext),
) {
    fun renderConf(
        writer: RustWriter,
        testCase: SmokeTestCase,
    ) {
        writer.rust("let conf = crate::config::Builder::new()")
        writer.indent()
        writer.rust(".behavior_version(crate::config::BehaviorVersion::latest())")

        val vendorParams = AwsSmokeTestModel.getAwsVendorParams(testCase)
        vendorParams.orNull()?.let { params ->
            writer.rust(".region(crate::config::Region::new(${params.region.dq()}))")
//            writer.rust(".region(crate::config::Region::new(${params.sigv4aRegionSet.orElse(listOf()).joinToString(", ").dq()}))")
//            writer.rust(".use_account_id_routing(${params.useAccountIdRouting()})")
            writer.rust(".use_dual_stack(${params.useDualstack()})")
            writer.rust(".use_fips(${params.useFips()})")
            params.uri.orNull()?.let { writer.rust(".endpoint_url($it)") }
        }

        val s3VendorParams = AwsSmokeTestModel.getS3VendorParams(testCase)
        s3VendorParams.orNull()?.let { params ->
            writer.rust(".accelerate_(${params.useAccelerate()})")
//             writer.rust(".use_global_endpoint_(${params.useGlobalEndpoint()})")
            writer.rust(".force_path_style_(${params.forcePathStyle()})")
            writer.rust(".use_arn_region(${params.useArnRegion()})")
            writer.rust(".disable_multi_region_access_points(${params.useMultiRegionAccessPoints().not()})")
        }

        writer.rust(".build();")
        writer.dedent()
    }

    fun renderInput(
        writer: RustWriter,
        operationShape: OperationShape,
        inputShape: StructureShape,
        data: Optional<ObjectNode>,
        headers: Map<String, String> = mapOf(),
        ctx: Ctx = Ctx(),
    ) {
        val operationBuilderName =
            FluentClientGenerator.clientOperationFnName(operationShape, codegenContext.symbolProvider)

        writer.rust("let res = client.$operationBuilderName()")
        writer.indent()
        data.orNull()?.let {
            renderStructureMembers(writer, inputShape, it, headers, ctx)
        }
        writer.rust(".send().await;")
        writer.dedent()
    }

    fun renderExpectation(
        writer: RustWriter,
        expectation: Expectation,
    ) {
        if (expectation.isSuccess) {
            writer.rust("""res.expect("request should succeed");""")
        } else if (expectation.isFailure) {
            writer.rust("""res.expect_err("request should fail");""")
        }
    }
}
