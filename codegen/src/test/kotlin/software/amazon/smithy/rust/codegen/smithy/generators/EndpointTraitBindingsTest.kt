/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.smithy.CodegenVisitor
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.customize.CombinedCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.customize.RequiredCustomizations
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.TokioTest
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.generatePluginContext
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.testutil.unitTest
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.codegen.util.runCommand
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
            operationShape.expectTrait(EndpointTrait::class.java)
        )
        val project = TestWorkspace.testProject()
        project.withModule(RustModule.default("test", visibility = Visibility.PRIVATE)) {
            it.rust(
                """
                struct GetStatusInput {
                    foo: Option<String>
                }
                """
            )
            it.implBlock(model.lookup("test#GetStatusInput"), sym) {
                it.rustBlock(
                    "fn endpoint_prefix(&self) -> std::result::Result<#T::endpoint::EndpointPrefix, #T>",
                    TestRuntimeConfig.smithyHttp(),
                    TestRuntimeConfig.operationBuildError()
                ) {
                    endpointBindingGenerator.render(this, "self")
                }
            }
            it.unitTest(
                "valid_prefix",
                """
                let inp = GetStatusInput { foo: Some("test_value".to_string()) };
                let prefix = inp.endpoint_prefix().unwrap();
                assert_eq!(prefix.as_str(), "test_valuea.data.");
                """
            )
            it.unitTest(
                "invalid_prefix",
                """
                // not a valid URI component
                let inp = GetStatusInput { foo: Some("test value".to_string()) };
                inp.endpoint_prefix().expect_err("invalid uri component");
                """
            )

            it.unitTest(
                "unset_prefix",
                """
                // unset is invalid
                let inp = GetStatusInput { foo: None };
                inp.endpoint_prefix().expect_err("invalid uri component");
                """
            )

            it.unitTest(
                "empty_prefix",
                """
                // empty is invalid
                let inp = GetStatusInput { foo: Some("".to_string()) };
                inp.endpoint_prefix().expect_err("empty label is invalid");
                """
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
        val (ctx, testDir) = generatePluginContext(model)
        val moduleName = ctx.settings.expectStringMember("module").value.replace('-', '_')
        val codegenDecorator = object : RustCodegenDecorator<ClientCodegenContext> {
            override val name: String = "add tests"
            override val order: Byte = 0

            override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
                rustCrate.withFile("tests/validate_errors.rs") {
                    TokioTest.render(it)
                    it.rust(
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
                        """
                    )
                }
            }
        }
        val combinedCodegenDecorator: CombinedCodegenDecorator<ClientCodegenContext> =
            CombinedCodegenDecorator.fromClasspath(ctx, RequiredCustomizations()).withDecorator(codegenDecorator)
        val visitor = CodegenVisitor(ctx, combinedCodegenDecorator)
        visitor.execute()
        "cargo test".runCommand(testDir)
    }
}
