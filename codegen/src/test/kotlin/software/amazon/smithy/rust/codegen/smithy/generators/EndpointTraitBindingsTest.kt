/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.CodegenVisitor
import software.amazon.smithy.rust.codegen.smithy.customize.CombinedCodegenDecorator
import software.amazon.smithy.rust.codegen.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.generatePluginContext
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.testutil.unitTest
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.codegen.util.runCommand
import kotlin.io.path.ExperimentalPathApi
import kotlin.io.path.createDirectory
import kotlin.io.path.writeText

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
        project.withModule(RustModule.default("test", false)) {
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
                    rust(".map_err(|e|#T::SerializationError(e.into()))", TestRuntimeConfig.operationBuildError())
                }
            }
            it.unitTest(
                """
                let inp = GetStatusInput { foo: Some("test_value".to_string()) };
                let prefix = inp.endpoint_prefix().unwrap();
                assert_eq!(prefix.as_str(), "test_valuea.data.");
            """
            )
            it.unitTest(
                """
                    // not a valid URI component
                let inp = GetStatusInput { foo: Some("test value".to_string()) };
                inp.endpoint_prefix().expect_err("invalid uri component");
            """
            )

            it.unitTest(
                """
                // unset is invalid
                let inp = GetStatusInput { foo: None };
                inp.endpoint_prefix().expect_err("invalid uri component");
            """
            )

            it.unitTest(
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
        val visitor = CodegenVisitor(ctx, CombinedCodegenDecorator.fromClasspath(ctx))
        val moduleName = ctx.settings.expectStringMember("module").value.replace('-', '_')
        visitor.execute()
        testDir.resolve("tests").createDirectory()
        testDir.resolve("tests/validate_errors.rs").writeText(
            """
                #[test]
                fn test_endpoint_prefix() {
                    let conf = $moduleName::Config::builder().build();
                    $moduleName::operation::SayHello::builder()
                        .greeting("hey there!").build().expect("input is valid")
                        .make_operation(&conf).expect_err("no spaces or exclamation points in ep prefixes");
                    let op = $moduleName::operation::SayHello::builder()
                        .greeting("hello")
                        .build().expect("valid operation")
                        .make_operation(&conf).expect("hello is a valid prefix");
                    let op_conf = op.config();
                    let prefix = op_conf.get::<smithy_http::endpoint::EndpointPrefix>()
                        .expect("prefix should be in config")
                        .as_str();
                    assert_eq!(prefix, "test123.hello.");
                }
        """
        )
        "cargo test".runCommand(testDir)
    }
}
