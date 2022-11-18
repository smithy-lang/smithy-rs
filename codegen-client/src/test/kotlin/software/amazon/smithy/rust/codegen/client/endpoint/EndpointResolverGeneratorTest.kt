/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.endpoint

import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.MethodSource
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rulesengine.language.Endpoint
import software.amazon.smithy.rulesengine.language.syntax.expr.Expression
import software.amazon.smithy.rulesengine.language.syntax.expr.Literal
import software.amazon.smithy.rulesengine.testutil.TestDiscovery
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointParamsGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointResolverGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTestGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.NativeSmithyFunctions
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.awsStandardLib
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import java.util.stream.Stream

class EndpointResolverGeneratorTest {
    companion object {
        @JvmStatic
        fun testSuites(): Stream<TestDiscovery.RulesTestSuite> = TestDiscovery().testSuites()
    }

    // for tests, load partitions.json from smithy—for real usage, this file will be inserted at codegen time
    private val partitionsJson =
        Node.parse(
            this::class.java.getResource("/software/amazon/smithy/rulesengine/language/partitions.json").readText(),
        )

    @ParameterizedTest(name = "{0}")
    @MethodSource("testSuites")
    fun `generate all rulesets`(suite: TestDiscovery.RulesTestSuite) {
        if (!suite.toString().contains("hostable")) {
            // return
        }
        val project = TestWorkspace.testProject()
        suite.ruleSet().typecheck()
        // try {
        project.lib {
            val ruleset = EndpointResolverGenerator(
                NativeSmithyFunctions + awsStandardLib(TestRuntimeConfig, partitionsJson),
                TestRuntimeConfig,
            ).generateResolverStruct(suite.ruleSet())
            val testGenerator = EndpointTestGenerator(
                suite.testSuite(),
                paramsType = EndpointParamsGenerator(suite.ruleSet().parameters).paramsStruct(),
                resolverType = ruleset,
                suite.ruleSet().parameters,
                TestRuntimeConfig,
            )
            testGenerator.generate()(this)
        }
        // } finally {
        project.compileAndTest()
        //       }
    }

    @Test
    fun generateEndpoints() {
        val endpoint = Endpoint.builder().url(Expression.of("https://{Region}.amazonaws.com"))
            .addHeader("x-amz-test", listOf(Literal.of("header-value")))
            .addAuthScheme(
                "sigv4",
                hashMapOf("signingName" to Literal.of("service"), "signingScope" to Literal.of("{Region}")),
            )
            .build()
        val generator = EndpointResolverGenerator(listOf(), TestRuntimeConfig)
        TestWorkspace.testProject().unitTest {
            rustTemplate(
                """
                // create a local for region to generate against
                let region = "us-east-1";
                let endpoint = #{endpoint:W};

                """,
                "endpoint" to generator.generateEndpoint(endpoint),
            )
        }.compileAndTest()
    }
}
