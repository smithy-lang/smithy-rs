/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.smoketests.model.AwsSmokeTestModel
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.cfg
import software.amazon.smithy.rust.codegen.core.rustlang.AttributeKind
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.containerDocs
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.PublicImportSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.smoketests.traits.Expectation
import software.amazon.smithy.smoketests.traits.SmokeTestCase
import software.amazon.smithy.smoketests.traits.SmokeTestsTrait
import java.util.Optional
import java.util.logging.Logger

class SmokeTestsDecorator : ClientCodegenDecorator {
    override val name: String = "SmokeTests"
    override val order: Byte = 0
    private val logger: Logger = Logger.getLogger(javaClass.name)

    private fun isSmokeTestSupported(smokeTestCase: SmokeTestCase): Boolean {
        AwsSmokeTestModel.getAwsVendorParams(smokeTestCase)?.orNull()?.let { vendorParams ->
            if (vendorParams.sigv4aRegionSet.isPresent) {
                logger.warning("skipping smoketest `${smokeTestCase.id}` with unsupported vendorParam `sigv4aRegionSet`")
                return false
            }
        }
        AwsSmokeTestModel.getS3VendorParams(smokeTestCase)?.orNull()?.let { s3VendorParams ->
            if (s3VendorParams.useGlobalEndpoint()) {
                logger.warning("skipping smoketest `${smokeTestCase.id}` with unsupported vendorParam `useGlobalEndpoint`")
                return false
            }
        }

        return true
    }

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        // Get all operations with smoke tests
        val smokeTestedOperations =
            codegenContext.model.getOperationShapesWithTrait(SmokeTestsTrait::class.java).toList()
        val supportedTests =
            smokeTestedOperations.map { operationShape ->
                // filter out unsupported smoke tests, logging a warning for each one, and sort the remaining tests by
                // case ID. This ensures deterministic rendering, meaning the test methods are always rendered in a
                // consistent order.
                val testCases =
                    operationShape.expectTrait<SmokeTestsTrait>().testCases.filter { smokeTestCase ->
                        isSmokeTestSupported(smokeTestCase)
                    }.sortedBy { smokeTestCase -> smokeTestCase.id }

                operationShape to testCases
            }
                // filter out operations with no supported smoke tests
                .filter { (_, testCases) -> testCases.isNotEmpty() }
                // Similar to sorting test cases above, sort operations by name to ensure consistent ordering.
                .sortedBy { (operationShape, _) -> operationShape.id.name }
        // Return if there are no supported smoke tests across all operations
        if (supportedTests.isEmpty()) return

        rustCrate.integrationTest("smoketests") {
            // Don't run the tests in this module unless `RUSTFLAGS="--cfg smoketests"` is passed.
            Attribute(cfg("smoketests")).render(this, AttributeKind.Inner)

            containerDocs(
                """
                The tests in this module run against live AWS services. As such,
                they are disabled by default. To enable them, run the tests with

                ```sh
                RUSTFLAGS="--cfg smoketests" cargo test.
                ```""",
            )

            val model = codegenContext.model
            val moduleUseName = codegenContext.moduleUseName()
            rust("use $moduleUseName::{ Client, config };")

            for ((operationShape, testCases) in supportedTests) {
                val operationName = operationShape.id.name.toSnakeCase()
                val operationInput = operationShape.inputShape(model)

                docs("Smoke tests for the `$operationName` operation")

                for (testCase in testCases) {
                    Attribute.TokioTest.render(this)
                    this.rustBlock("async fn test_${testCase.id.toSnakeCase()}()") {
                        val instantiator = SmokeTestsInstantiator(codegenContext)
                        instantiator.renderConf(this, testCase)
                        rust("let client = Client::from_conf(conf);")
                        instantiator.renderInput(this, operationShape, operationInput, testCase.params)
                        instantiator.renderExpectation(this, model, testCase.expectation)
                    }
                }
            }
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
    PublicImportSymbolProvider(codegenContext.symbolProvider, codegenContext.moduleUseName()),
    codegenContext.model,
    codegenContext.runtimeConfig,
    SmokeTestsBuilderKindBehavior(codegenContext),
) {
    fun renderConf(
        writer: RustWriter,
        testCase: SmokeTestCase,
    ) {
        writer.rust(
            "let config = #{T}::load_defaults(config::BehaviorVersion::latest()).await;",
            AwsCargoDependency.awsConfig(codegenContext.runtimeConfig).toType(),
        )
        writer.rust("let conf = config::Config::from(&config).to_builder()")
        writer.indent()

        // TODO(https://github.com/smithy-lang/smithy-rs/issues/3776) Once Account ID routing is supported,
        //  reflect the config setting here, especially to disable it if needed, as it is enabled by default in
        //  `AwsVendorParams`.

        val vendorParams = AwsSmokeTestModel.getAwsVendorParams(testCase)
        vendorParams.orNull()?.let { params ->
            writer.rust(".region(config::Region::new(${params.region.dq()}))")
            writer.rust(".use_dual_stack(${params.useDualstack()})")
            writer.rust(".use_fips(${params.useFips()})")
            params.uri.orNull()?.let { writer.rust(".endpoint_url($it)") }
        }

        val s3VendorParams = AwsSmokeTestModel.getS3VendorParams(testCase)
        s3VendorParams.orNull()?.let { params ->
            writer.rust(".accelerate_(${params.useAccelerate()})")
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
        model: Model,
        expectation: Expectation,
    ) {
        if (expectation.isSuccess) {
            writer.rust("""res.expect("request should succeed");""")
        } else if (expectation.isFailure) {
            val expectedErrShape = expectation.failure.orNull()?.errorId?.orNull()
            println(expectedErrShape)
            if (expectedErrShape != null) {
                val failureShape = model.expectShape(expectedErrShape)
                val errName = codegenContext.symbolProvider.toSymbol(failureShape).name.toSnakeCase()
                writer.rust(
                    """
                    let err = res.expect_err("request should fail");
                    let err = err.into_service_error();
                    assert!(err.is_$errName())
                    """,
                )
            } else {
                writer.rust("""res.expect_err("request should fail");""")
            }
        }
    }
}
