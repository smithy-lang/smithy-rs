/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.MethodSource
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rulesengine.language.Endpoint
import software.amazon.smithy.rulesengine.language.eval.Scope
import software.amazon.smithy.rulesengine.language.eval.Type
import software.amazon.smithy.rulesengine.language.syntax.expr.Expression
import software.amazon.smithy.rulesengine.language.syntax.expr.Literal
import software.amazon.smithy.rulesengine.testutil.TestDiscovery
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointParamsGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointResolverGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.EndpointTestGenerator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.SmithyEndpointsStdLib
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.awsStandardLib
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import java.util.stream.Stream

class EndpointResolverGeneratorTest {
    companion object {
        @JvmStatic
        fun testSuites(): Stream<TestDiscovery.RulesTestSuite> =
            TestDiscovery().testSuites().map { it.ruleSet().typecheck(); it }
    }

    // for tests, load partitions.json from smithy—for real usage, this file will be inserted at codegen time
    private val partitionsJson =
        Node.parse(
            this::class.java.getResource("/software/amazon/smithy/rulesengine/language/partitions.json")?.readText()
                ?: throw CodegenException("partitions.json was not present in smithy bundle"),
        )

    @ParameterizedTest(name = "{0}")
    @MethodSource("testSuites")
    fun `generate all rulesets`(suite: TestDiscovery.RulesTestSuite) {
        // snippet to only run one ruleset during tests
        if (!suite.toString().contains("hostable")) {
            // return
        }
        val project = TestWorkspace.testProject()
        val context = testClientCodegenContext()
        suite.ruleSet().typecheck()
        project.lib {
            val ruleset = EndpointResolverGenerator(
                context,
                SmithyEndpointsStdLib + awsStandardLib(TestRuntimeConfig, partitionsJson),
            ).defaultEndpointResolver(suite.ruleSet())
            val testGenerator = EndpointTestGenerator(
                suite.testSuite().testCases,
                paramsType = EndpointParamsGenerator(context, suite.ruleSet().parameters).paramsStruct(),
                resolverType = ruleset,
                suite.ruleSet().parameters,
                codegenContext = testClientCodegenContext(model = Model.builder().build()),
                endpointCustomizations = listOf(),
            )
            testGenerator.generate()(this)
        }
        project.compileAndTest(runClippy = true)
    }

    @Test
    fun `only include actually used functions in endpoints lib`() {
        testSuites().map { it.ruleSet().sourceLocation.filename }.forEach { println(it) }
        val suite =
            testSuites().filter { it.ruleSet().sourceLocation.filename.endsWith("/uri-encode.json") }.findFirst()
                .orElseThrow()
        val project = TestWorkspace.testProject()
        val context = testClientCodegenContext()
        suite.ruleSet().typecheck()
        project.lib {
            val ruleset = EndpointResolverGenerator(
                context,
                SmithyEndpointsStdLib + awsStandardLib(TestRuntimeConfig, partitionsJson),
            ).defaultEndpointResolver(suite.ruleSet())
            val testGenerator = EndpointTestGenerator(
                suite.testSuite().testCases,
                paramsType = EndpointParamsGenerator(context, suite.ruleSet().parameters).paramsStruct(),
                resolverType = ruleset,
                suite.ruleSet().parameters,
                codegenContext = testClientCodegenContext(Model.builder().build()),
                endpointCustomizations = listOf(),
            )
            testGenerator.generate()(this)
        }
        project.compileAndTest()
        project.generatedFiles()
            .filter { it.toAbsolutePath().toString().contains("endpoint_lib/") }
            .map { it.fileName.toString() }.sorted() shouldBe listOf(
            "diagnostic.rs", "uri_encode.rs",
        )
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
        val scope = Scope<Type>()
        scope.insert("Region", Type.string())
        endpoint.typeCheck(scope)
        val context = testClientCodegenContext()
        val generator = EndpointResolverGenerator(context, listOf())
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
