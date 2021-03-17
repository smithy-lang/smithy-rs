/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.EndpointTrait
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.customize.CombinedCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.generatePluginContext
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.testutil.unitTest
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.codegen.util.runCommand

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
        @endpoint(hostPrefix: "{foo}.data")
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
            operationShape,
            operationShape.expectTrait(EndpointTrait::class.java)
        )
        val project = TestWorkspace.testProject()
        project.withModule(RustModule.default("test", false)) {
            it.rust(
                """
                struct GetStatusInput {
                    foo: String
                }
            """
            )
            it.implBlock(model.lookup("test#GetStatusInput"), sym) {
                it.rustBlock(
                    "fn endpoint_prefix(&self) -> #T::endpoint::EndpointPrefix",
                    TestRuntimeConfig.smithyHttp()
                ) {
                    endpointBindingGenerator.render(this, "self")
                    rust(".unwrap()")
                }
            }
            it.unitTest(
                """
                let inp = GetStatusInput { foo: "test_value".to_string() };
                let prefix = inp.endpoint_prefix();
                assert_eq!(prefix.as_str(), "test_value.data");
            """
            )
        }

        project.compileAndTest()
    }

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
        val (ctx, testDir) = generatePluginContext(model, "com.example#TestService")
        val testDecorator = object : RustCodegenDecorator {
            override val name: String = "Test"
            override val order: Byte = 0
            override fun extras(protocolConfig: ProtocolConfig, rustCrate: RustCrate) {
                rustCrate.withModule(RustModule.default("test", public = false)) {
                    it.unitTest(
                        """
                let conf = crate::Config::builder().build();
                crate::operation::SayHello::builder().greeting("hey there!").build(&conf).expect_err("no spaces or exclamation points in ep prefixes");
                let op = crate::operation::SayHello::builder().greeting("hello").build(&conf).expect("hello is a valid prefix");
                let op_conf = op.config();
                let prefix = op_conf.get::<smithy_http::endpoint::EndpointPrefix>()
                    .expect("prefix should be in config")
                    .as_str();
                assert_eq!(prefix, "test123.hello.");

            """
                    )
                }
            }
        }
        val visitor = CodegenVisitor(ctx, CombinedCodegenDecorator.fromClasspath(ctx).withDecorator(testDecorator))
        visitor.execute()
        "cargo test".runCommand(testDir)
    }
}
