/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.MethodSource
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.testutil.EndpointTestDiscovery
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest

class EndpointResolverGeneratorTest {
    companion object {
        val testCases =
            listOf(
                "default-values.smithy",
                "deprecated-param.smithy",
                "duplicate-param.smithy",
                "get-attr-type-inference.smithy",
                "headers.smithy",
                "minimal-ruleset.smithy",
                "parse-url.smithy",
                "substring.smithy",
                "uri-encode.smithy",
                "valid-hostlabel.smithy",
                "valid-model.smithy",
            )

        @JvmStatic
        fun testSuites(): List<Model> {
            return EndpointTestDiscovery().testCases("ruleset-")
        }

        @JvmStatic
        fun testSuitesBdd(): List<Model> {
            return EndpointTestDiscovery().testCases("bdd-")
        }
    }

    @Test
    fun `test`() {
        `generate all rulesets`(testSuites()[0])
    }

    // for tests, load partitions.json from smithy—for real usage, this file will be inserted at codegen time
    //
    // private val partitionsJson =
    //     Node.parse(
    //         this::class.java.getResource("/software/amazon/smithy/rulesengine/language/partitions.json")?.readText()
    //             ?: throw CodegenException("partitions.json was not present in smithy bundle"),
    //     )

    @ParameterizedTest(name = "{0}")
    @MethodSource("testSuites")
    fun `generate all rulesets`(suite: Model) {
        // snippet to only run one ruleset during tests
        // if (!suite.toString().contains("hostable")) {
        // return
        // }

        clientIntegrationTest(suite)
    }

    @ParameterizedTest(name = "{0}")
    @MethodSource("testSuitesBdd")
    fun `generate all BDD tests`(suite: Model) {
        // snippet to only run one ruleset during tests
        // if (!suite.toString().contains("hostable")) {
        // return
        // }

        val namespace =
            suite.getShapeIds().stream()
                .map { obj: ShapeId -> obj.namespace.toString() }
                .filter { it.contains("endpointrules") }
                .toList()
                .distinct()

        if (namespace[0].contains("split")) {
            println("NAMESPACE: $namespace")
            clientIntegrationTest(suite)
        }
    }

    /*
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
            .putHeader("x-amz-test", listOf(Literal.of("header-value")))
            .addAuthScheme(
                "sigv4",
                hashMapOf("signingName" to Literal.of("service"), "signingScope" to Literal.of("{Region}")),
            )
            .build()
        val scope = Scope<Type>()
        scope.insert("Region", Type.stringType())
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
    }*/
}
