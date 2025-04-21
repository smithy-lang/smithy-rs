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
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameters
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointRulesetIndex
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.cfg
import software.amazon.smithy.rust.codegen.core.rustlang.AttributeKind
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.containerDocs
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
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
import kotlin.jvm.optionals.getOrElse

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
        val smokeTestedOperations = operationToTestCases(codegenContext.model)
        val supportedTests =
            smokeTestedOperations.map { (operationShape, testCases) ->
                // filter out unsupported smoke tests, logging a warning for each one, and sort the remaining tests by
                // case ID. This ensures deterministic rendering, meaning the test methods are always rendered in a
                // consistent order.
                testCases.filter { smokeTestCase ->
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
            renderPrologue(codegenContext.moduleUseName(), this)

            for ((operationShape, testCases) in supportedTests) {
                docs("Smoke tests for the `${operationShape.id.name.toSnakeCase()}` operation")
                val instantiator =
                    SmokeTestsInstantiator(
                        codegenContext, operationShape,
                        configBuilderInitializer = { ->
                            writable {
                                rustTemplate(
                                    """
                                    let config = #{awsConfig}::load_defaults(config::BehaviorVersion::latest()).await;
                                    let conf = config::Config::from(&config).to_builder()
                                    """,
                                    "awsConfig" to AwsCargoDependency.awsConfig(codegenContext.runtimeConfig).toType(),
                                )
                            }
                        },
                    )
                for (testCase in testCases) {
                    Attribute.TokioTest.render(this)
                    this.rustBlock("async fn test_${testCase.id.toSnakeCase()}()") {
                        instantiator.render(this, testCase)
                    }
                }
            }
        }
    }
}

fun renderPrologue(
    moduleUseName: String,
    writer: RustWriter,
) = writer.apply {
    // Don't run the tests in this module unless `RUSTFLAGS="--cfg smoketests"` is passed.
    Attribute(cfg("smoketests")).render(this, AttributeKind.Inner)

    containerDocs(
        """
        The tests in this module run against live AWS services. As such,
        they are disabled by default. To enable them, run the tests with

        ```sh
        RUSTFLAGS="--cfg smoketests" cargo test
        ```
        """,
    )

    rust("use $moduleUseName::{Client, config};")
}

fun operationToTestCases(model: Model) =
    model.getOperationShapesWithTrait(SmokeTestsTrait::class.java).toList().map { operationShape ->
        operationShape to operationShape.expectTrait<SmokeTestsTrait>().testCases
    }

class SmokeTestsBuilderKindBehavior(val codegenContext: CodegenContext) : Instantiator.BuilderKindBehavior {
    override fun hasFallibleBuilder(shape: StructureShape): Boolean =
        BuilderGenerator.hasFallibleBuilder(shape, codegenContext.symbolProvider)

    override fun setterName(memberShape: MemberShape): String = memberShape.setterName()

    override fun doesSetterTakeInOption(memberShape: MemberShape): Boolean = true
}

class SmokeTestsInstantiator(
    codegenContext: ClientCodegenContext, private val operationShape: OperationShape,
    private val configBuilderInitializer: () -> Writable,
) : Instantiator(
        PublicImportSymbolProvider(codegenContext.symbolProvider, codegenContext.moduleUseName()),
        codegenContext.model,
        codegenContext.runtimeConfig,
        SmokeTestsBuilderKindBehavior(codegenContext),
    ) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider

    // Get list of the built-ins actually included in the model
    private val builtInParamNames: List<String> by lazy {
        val index = EndpointRulesetIndex.of(codegenContext.model)
        val rulesOrNull = index.endpointRulesForService(codegenContext.serviceShape)
        val builtInParams: Parameters = (rulesOrNull?.parameters ?: Parameters.builder().build())
        val temp: MutableList<String> = mutableListOf()
        builtInParams.forEach { temp.add(it.builtIn.getOrElse { "" }) }
        temp
    }
    private val fipsName = "AWS::UseFIPS"
    private val dualStackName = "AWS::UseDualStack"
    private val rc = codegenContext.runtimeConfig

    fun render(
        writer: RustWriter,
        testCase: SmokeTestCase,
    ) = writer.apply {
        renderConf(this, testCase)
        rust("let client = Client::from_conf(conf);")
        renderInput(this, testCase.params)
        renderExpectation(this, testCase.expectation)
    }

    private fun renderConf(
        writer: RustWriter,
        testCase: SmokeTestCase,
    ) = writer.apply {
        rustTemplate("#{config_builder_initializer}", "config_builder_initializer" to configBuilderInitializer())
        indent()

        // TODO(https://github.com/smithy-lang/smithy-rs/issues/3863) When re-enabling service smoke tests,
        //  include the config setting for account ID, especially to disable it if needed, as it is enabled
        //  by default in `AwsVendorParams`.

        val vendorParams = AwsSmokeTestModel.getAwsVendorParams(testCase)
        vendorParams.orNull()?.let { params ->
            rustTemplate(
                ".region(#{Region}::new(${params.region.dq()}))",
                "Region" to AwsRuntimeType.awsTypes(rc).resolve("region::Region"),
            )

            // The `use_dual_stack` and `use_fips` fields will only exist on the endpoint params if they built-ins are
            // included in the model, so we check for that before setting them.
            if (builtInParamNames.contains(dualStackName)) {
                rust(".use_dual_stack(${params.useDualstack()})")
            }
            if (builtInParamNames.contains(fipsName)) {
                rust(".use_fips(${params.useFips()})")
            }

            params.uri.orNull()?.let { rust(".endpoint_url($it)") }
        }

        val s3VendorParams = AwsSmokeTestModel.getS3VendorParams(testCase)
        s3VendorParams.orNull()?.let { params ->
            rust(".accelerate_(${params.useAccelerate()})")
            rust(".force_path_style_(${params.forcePathStyle()})")
            rust(".use_arn_region(${params.useArnRegion()})")
            rust(".disable_multi_region_access_points(${params.useMultiRegionAccessPoints().not()})")
        }

        rust(".build();")
        dedent()
    }

    private fun renderInput(
        writer: RustWriter,
        data: Optional<ObjectNode>,
        headers: Map<String, String> = mapOf(),
        ctx: Ctx = Ctx(),
    ) = writer.apply {
        val operationBuilderName =
            FluentClientGenerator.clientOperationFnName(operationShape, symbolProvider)
        val inputShape = operationShape.inputShape(model)

        rust("let res = client.$operationBuilderName()")
        indent()
        data.orNull()?.let {
            renderStructureMembers(writer, inputShape, it, headers, ctx)
        }
        rust(".send().await;")
        dedent()
    }

    private fun renderExpectation(
        writer: RustWriter,
        expectation: Expectation,
    ) = writer.apply {
        if (expectation.isSuccess) {
            rust("""res.expect("request should succeed");""")
        } else if (expectation.isFailure) {
            val expectedErrShape = expectation.failure.orNull()?.errorId?.orNull()
            println(expectedErrShape)
            if (expectedErrShape != null) {
                val failureShape = model.expectShape(expectedErrShape)
                val errName = symbolProvider.toSymbol(failureShape).name.toSnakeCase()
                rust(
                    """
                    let err = res.expect_err("request should fail");
                    let err = err.into_service_error();
                    assert!(err.is_$errName())
                    """,
                )
            } else {
                rust("""res.expect_err("request should fail");""")
            }
        }
    }
}
