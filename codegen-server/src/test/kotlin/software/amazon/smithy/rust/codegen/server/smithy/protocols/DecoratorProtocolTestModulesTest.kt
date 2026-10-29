/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolTestGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.TestCase
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/**
 * Supplies replacement protocol generators through a codegen decorator and generates a service
 * with protocol tests for multiple protocols. It compiles and runs the generated test modules
 * with legacy and schema serde, checking that replacing a generator does not overwrite another
 * protocol's tests or produce conflicting module definitions.
 */
internal class DecoratorProtocolTestModulesTest {
    @Test
    fun `replacement generators preserve independent protocol test modules`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.decoratormodules
            use aws.protocols#restJson1
            use aws.protocols#awsJson1_1
            use smithy.test#httpRequestTests
            @restJson1 @awsJson1_1 service Decorated { version: "1", operations: [Echo] }
            @http(method: "POST", uri: "/echo")
            @httpRequestTests([
                { id: "RestRequest", protocol: restJson1, method: "POST", uri: "/echo",
                  headers: {"Content-Type": "application/json"}, body: "{}", params: {} },
                { id: "RpcRequest", protocol: awsJson1_1, method: "POST", uri: "/",
                  headers: {"Content-Type": "application/x-amz-json-1.1", "X-Amz-Target": "Decorated.Echo"},
                  body: "{}", params: {} }
            ])
            operation Echo { input := {}, output := {} }
            """.asSmithyModel()
        val replacement =
            object : ServerCodegenDecorator {
                override val name = "Forward protocol test generation through a replacement"
                override val order: Byte = 0

                override fun protocolTestGenerator(
                    codegenContext: ServerCodegenContext,
                    baseGenerator: ProtocolTestGenerator,
                ): ProtocolTestGenerator =
                    object : ProtocolTestGenerator() {
                        override val codegenContext = baseGenerator.codegenContext
                        override val protocolSupport = baseGenerator.protocolSupport
                        override val operationShape = baseGenerator.operationShape
                        override val appliesTo = baseGenerator.appliesTo
                        override val logger = baseGenerator.logger
                        override val expectFail = baseGenerator.expectFail
                        override val brokenTests = baseGenerator.brokenTests
                        override val generateOnly = baseGenerator.generateOnly
                        override val disabledTests = baseGenerator.disabledTests

                        override fun RustWriter.renderAllTestCases(allTests: List<TestCase>) {
                            val writer = this
                            with(baseGenerator) { writer.renderAllTestCases(allTests) }
                        }
                    }
            }
        for (schemaSerde in listOf(false, true)) {
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schemaSerde).withHttp1x().toObjectNode(),
                    overrideTestDir = Files.createTempDirectory("decorator-protocol-tests-$schemaSerde-").toFile(),
                    cargoCommand = "cargo test --quiet --lib",
                ),
                additionalDecorators = listOf(replacement),
                testCoverage = HttpTestType.Default,
            )
        }
    }
}
