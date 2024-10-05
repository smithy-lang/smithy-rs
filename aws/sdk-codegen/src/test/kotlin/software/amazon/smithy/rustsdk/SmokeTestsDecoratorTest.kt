/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenVisitor
import software.amazon.smithy.rust.codegen.client.smithy.customizations.NoAuthDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.CombinedClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.RequiredCustomizations
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointsDecorator
import software.amazon.smithy.rust.codegen.client.testutil.ClientDecoratableBuildPlugin
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

class SmokeTestsDecoratorTest {
    companion object {
        val imports = """
            namespace test

            use aws.api#service
            use smithy.test#smokeTests
            use aws.auth#sigv4
            use aws.protocols#restJson1
            use smithy.rules#endpointRuleSet
        """
        val traitsWithAllBuiltIns =
            """
            @service(sdkId: "dontcare")
            @restJson1
            @sigv4(name: "dontcare")
            @auth([sigv4])
            @endpointRuleSet({
                "version": "1.0",
                "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
                "parameters": {
                    "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
                    "Endpoint": {
                      "builtIn": "SDK::Endpoint",
                      "required": false,
                      "documentation": "Override the endpoint used to send this request",
                      "type": "String"
                    },
                    "UseFIPS": {
                      "builtIn": "AWS::UseFIPS",
                      "required": true,
                      "default": false,
                      "type": "Boolean"
                    },
                    "UseDualStack": {
                      "builtIn": "AWS::UseDualStack",
                      "required": true,
                      "default": false,
                      "type": "Boolean"
                    },
                }
            })
            """

        val traitsWithNoDualStack =
            """
            @service(sdkId: "dontcare")
            @restJson1
            @sigv4(name: "dontcare")
            @auth([sigv4])
            @endpointRuleSet({
                "version": "1.0",
                "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
                "parameters": {
                    "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
                    "Endpoint": {
                      "builtIn": "SDK::Endpoint",
                      "required": false,
                      "documentation": "Override the endpoint used to send this request",
                      "type": "String"
                    },
                    "UseFIPS": {
                      "builtIn": "AWS::UseFIPS",
                      "required": true,
                      "default": false,
                      "type": "Boolean"
                    },
                }
            })
            """

        val traitsWithNoFips =
            """
            @service(sdkId: "dontcare")
            @restJson1
            @sigv4(name: "dontcare")
            @auth([sigv4])
            @endpointRuleSet({
                "version": "1.0",
                "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
                "parameters": {
                    "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
                    "Endpoint": {
                      "builtIn": "SDK::Endpoint",
                      "required": false,
                      "documentation": "Override the endpoint used to send this request",
                      "type": "String"
                    },
                    "UseDualStack": {
                      "builtIn": "AWS::UseDualStack",
                      "required": true,
                      "default": false,
                      "type": "Boolean"
                    },
                }
            })
            """

        val traitsWithNeither =
            """
            @service(sdkId: "dontcare")
            @restJson1
            @sigv4(name: "dontcare")
            @auth([sigv4])
            @endpointRuleSet({
                "version": "1.0",
                "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
                "parameters": {
                    "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
                }
            })
            """

        val serviceDef = """
            service TestService {
                version: "2023-01-01",
                operations: [SomeOperation]
            }

            @smokeTests([
                {
                    id: "SomeOperationSuccess",
                    params: {}
                    vendorParamsShape: "aws.test#AwsVendorParams",
                    vendorParams: {
                        region: "us-west-2"
                    }
                    expect: { success: {} }
                }
                {
                    id: "SomeOperationFailure",
                    params: {}
                    vendorParamsShape: "aws.test#AwsVendorParams",
                    vendorParams: {
                        region: "us-west-2"
                    }
                    expect: { failure: {} }
                }
                {
                    id: "SomeOperationFailureExplicitShape",
                    params: {}
                    vendorParamsShape: "aws.test#AwsVendorParams",
                    vendorParams: {
                        region: "us-west-2"
                    }
                    expect: {
                        failure: { errorId: FooException }
                    }
                }
            ])
            @http(uri: "/SomeOperation", method: "POST")
            @optionalAuth
            operation SomeOperation {
                input: SomeInput,
                output: SomeOutput,
                errors: [FooException]
            }

            @input
            structure SomeInput {}

            @output
            structure SomeOutput {}

            @error("server")
            structure FooException { }
        """
        val model = (imports + traitsWithAllBuiltIns + serviceDef).asSmithyModel(smithyVersion = "2")
        val modelWithNoDualStack = (imports + traitsWithNoDualStack + serviceDef).asSmithyModel(smithyVersion = "2")
        val modelWithNoFips = (imports + traitsWithNoFips + serviceDef).asSmithyModel(smithyVersion = "2")
        val modelWithNeitherBuiltIn = (imports + traitsWithNeither + serviceDef).asSmithyModel(smithyVersion = "2")
    }

    @Test
    fun smokeTestSdkCodegen() {
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/3863): Comment in once the issue has been resolved
        // testSmokeTestsWithModel(model)
    }

    @Test
    fun smokeTestSdkCodegenNoDualStack() {
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/3863): Comment in once the issue has been resolved
        // testSmokeTestsWithModel(modelWithNoDualStack)
    }

    @Test
    fun smokeTestSdkCodegenNoFips() {
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/3863): Comment in once the issue has been resolved
        // testSmokeTestsWithModel(modelWithNoFips)
    }

    @Test
    fun smokeTestSdkCodegenNeitherBuiltIn() {
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/3863): Comment in once the issue has been resolved
        // testSmokeTestsWithModel(modelWithNeitherBuiltIn)
    }

    fun testSmokeTestsWithModel(model: Model) {
        val codegenContext = testClientCodegenContext(model)
        val smokeTestedOperations = operationToTestCases(model)
        awsSdkIntegrationTest(
            model,
            buildPlugin = SdkSmokeTestsRustClientCodegenPlugin(),
            // `SdkSmokeTestsRustClientCodegenPlugin` only uses the minimal set of codegen decorators, which results
            // in a significant amount of unused code. This can cause `clippy` to fail with the `--deny warnings`
            // setting enabled by default in `.crate/config.toml` in test workspaces.
            // To work around this issue, we unset `RUSTFLAGS` to allow unused and dead code. To perform a compilation
            // only test, we don't need to set `mapOf(Pair("RUSTFLAGS", "--cfg smoketests"))` since that would
            // cause the tests to actually run (against a non-existent service) and fail.
            environment = mapOf(Pair("RUSTFLAGS", "")),
            test = { codegenContext, crate ->
                // It should compile. We can't run the tests because they don't target a real service.
                // They are skipped because the `smoketests` flag is unset for `rustc` in the `cargo test`
                // invocation specified by `awsIntegrationTestParams`.
                crate.integrationTest("smoketests") {
                    renderPrologue(codegenContext.moduleUseName(), this)
                    for ((shape, testCases) in smokeTestedOperations) {
                        val sut =
                            SmokeTestsInstantiator(
                                codegenContext, shape,
                                // We cannot use `aws_config::load_defaults`, but a default-constructed config builder
                                // will suffice for the test.
                                configBuilderInitializer = { ->
                                    writable {
                                        rust(
                                            """
                                            let conf = config::Builder::new().build();
                                            let params = ${codegenContext.moduleUseName()}::config::endpoint::Params::builder()
                                            """.trimIndent(),
                                        )
                                    }
                                },
                            )
                        for (testCase in testCases) {
                            tokioTest("test_${testCase.id.toSnakeCase()}") {
                                sut.render(this, testCase)
                            }
                        }
                    }
                }
            },
        )
    }
}

/**
 * A `ClientDecoratableBuildPlugin` that intentionally avoids including `codegenDecorator` on the classpath.
 *
 *  If we used `RustClientCodegenPlugin` on the classpath from this location, it would return decorators including
 *  `SmokeTestsDecorator` that ultimately pulls in the `aws-config` crate. This crate depends on runtime crates located
 *  in the `aws/sdk/build` directory, causing conflicts with runtime crates in `rust-runtime` or in `aws/rust-runtime`.
 *
 *  This class does not look at the classpath to prevent the inclusion of the `aws-config` crate. Instead, it uses the
 *  minimal set of codegen decorators to generate modules sufficient to compile smoke tests in a test
 *  workspace.
 */
class SdkSmokeTestsRustClientCodegenPlugin : ClientDecoratableBuildPlugin() {
    override fun getName(): String = "sdk-smoke-tests-rust-client-codegen"

    override fun executeWithDecorator(
        context: PluginContext,
        vararg decorator: ClientCodegenDecorator,
    ) {
        val codegenDecorator =
            CombinedClientCodegenDecorator(
                listOf(
                    EndpointsDecorator(),
                    AwsFluentClientDecorator(),
                    SdkConfigDecorator(),
                    NoAuthDecorator(),
                    RequiredCustomizations(),
                    *decorator,
                ),
            )

        ClientCodegenVisitor(context, codegenDecorator).execute()
    }
}
