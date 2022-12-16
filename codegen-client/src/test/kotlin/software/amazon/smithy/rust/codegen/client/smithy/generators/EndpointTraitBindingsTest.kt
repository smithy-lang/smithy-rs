/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.client.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.core.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.TokioTest
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import kotlin.io.path.ExperimentalPathApi

internal class EndpointTraitBindingsTest {
    @Test
    fun `generate correct format strings`() {
        val epTrait = EndpointTrait.builder().hostPrefix("{foo}.data").build()
        epTrait.prefixFormatString() shouldBe ("\"{foo}.data\"")
    }

    @Test
    fun `generate endpoint prefixes`() {
        val model = """
            namespace test
            @readonly
            @endpoint(hostPrefix: "{foo}a.data.")
            operation GetStatus {
                input: GetStatusInput,
            }
            structure GetStatusInput {
                @required
                @hostLabel
                foo: String
            }
        """.asSmithyModel()
        val operationShape: OperationShape = model.lookup("test#GetStatus")
        val sym = testSymbolProvider(model)
        val endpointBindingGenerator = EndpointTraitBindings(
            model,
            sym,
            TestRuntimeConfig,
            operationShape,
            operationShape.expectTrait(EndpointTrait::class.java),
        )
        val project = TestWorkspace.testProject()
        project.withModule(RustModule.private("test")) {
            rust(
                """
                struct GetStatusInput {
                    foo: Option<String>
                }
                """,
            )
            implBlock(model.lookup("test#GetStatusInput"), sym) {
                rustBlock(
                    "fn endpoint_prefix(&self) -> std::result::Result<#T::endpoint::EndpointPrefix, #T>",
                    RuntimeType.smithyHttp(TestRuntimeConfig),
                    TestRuntimeConfig.operationBuildError(),
                ) {
                    endpointBindingGenerator.render(this, "self")
                }
            }
            unitTest(
                "valid_prefix",
                """
                let inp = GetStatusInput { foo: Some("test_value".to_string()) };
                let prefix = inp.endpoint_prefix().unwrap();
                assert_eq!(prefix.as_str(), "test_valuea.data.");
                """,
            )
            unitTest(
                "invalid_prefix",
                """
                // not a valid URI component
                let inp = GetStatusInput { foo: Some("test value".to_string()) };
                inp.endpoint_prefix().expect_err("invalid uri component");
                """,
            )

            unitTest(
                "unset_prefix",
                """
                // unset is invalid
                let inp = GetStatusInput { foo: None };
                inp.endpoint_prefix().expect_err("invalid uri component");
                """,
            )

            unitTest(
                "empty_prefix",
                """
                // empty is invalid
                let inp = GetStatusInput { foo: Some("".to_string()) };
                inp.endpoint_prefix().expect_err("empty label is invalid");
                """,
            )
        }

        project.compileAndTest()
    }

    @ExperimentalPathApi
    @Test
    fun `endpoint integration test`() {
        val model = """
            namespace com.example
            use aws.protocols#awsJson1_0
            @awsJson1_0
            @aws.api#service(sdkId: "Test", endpointPrefix: "differentprefix")
            service TestService {
                operations: [SayHello],
                version: "1"
            }
            @endpoint(hostPrefix: "test123.{greeting}.")
            operation SayHello {
                input: SayHelloInput
            }
            structure SayHelloInput {
                @required
                @hostLabel
                greeting: String
            }
        """.asSmithyModel()
        clientIntegrationTest(model) { clientCodegenContext, rustCrate ->
            val moduleName = clientCodegenContext.moduleUseName()
            rustCrate.integrationTest("test_endpoint_prefix") {
                TokioTest.render(this)
                rust(
                    """
                    async fn test_endpoint_prefix() {
                        let conf = $moduleName::Config::builder().build();
                        $moduleName::operation::SayHello::builder()
                            .greeting("hey there!").build().expect("input is valid")
                            .make_operation(&conf).await.expect_err("no spaces or exclamation points in ep prefixes");
                        let op = $moduleName::operation::SayHello::builder()
                            .greeting("hello")
                            .build().expect("valid operation")
                            .make_operation(&conf).await.expect("hello is a valid prefix");
                        let properties = op.properties();
                        let prefix = properties.get::<aws_smithy_http::endpoint::EndpointPrefix>()
                            .expect("prefix should be in config")
                            .as_str();
                        assert_eq!(prefix, "test123.hello.");
                    }
                    """,
                )
            }
        }
    }
}
