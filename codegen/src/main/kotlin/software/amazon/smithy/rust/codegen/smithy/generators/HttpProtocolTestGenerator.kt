/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.protocoltests.traits.AppliesTo
import software.amazon.smithy.protocoltests.traits.HttpMessageTestCase
import software.amazon.smithy.protocoltests.traits.HttpRequestTestCase
import software.amazon.smithy.protocoltests.traits.HttpRequestTestsTrait
import software.amazon.smithy.protocoltests.traits.HttpResponseTestCase
import software.amazon.smithy.protocoltests.traits.HttpResponseTestsTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.escape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase
import java.util.logging.Logger

data class ProtocolSupport(
    val requestBodySerialization: Boolean,
    val responseDeserialization: Boolean,
    val errorDeserialization: Boolean
)

/**
 * Generate protocol tests for an operation
 */
class HttpProtocolTestGenerator(
    private val protocolConfig: ProtocolConfig,
    private val protocolSupport: ProtocolSupport,
    private val operationShape: OperationShape,
    private val writer: RustWriter
) {
    private val logger = Logger.getLogger(javaClass.name)

    private val inputShape = operationShape.inputShape(protocolConfig.model)
    private val outputShape = operationShape.outputShape(protocolConfig.model)
    private val operationSymbol = protocolConfig.symbolProvider.toSymbol(operationShape)
    private val operationIndex = OperationIndex.of(protocolConfig.model)

    private val instantiator = with(protocolConfig) {
        Instantiator(symbolProvider, model, runtimeConfig)
    }

    sealed class TestCase {
        abstract val testCase: HttpMessageTestCase

        data class RequestTest(override val testCase: HttpRequestTestCase) : TestCase()
        data class ResponseTest(override val testCase: HttpResponseTestCase, val targetShape: StructureShape) :
            TestCase()
    }

    fun render() {
        val requestTests = operationShape.getTrait(HttpRequestTestsTrait::class.java)
            .orNull()?.getTestCasesFor(AppliesTo.CLIENT).orEmpty().map { TestCase.RequestTest(it) }
        val responseTests = operationShape.getTrait(HttpResponseTestsTrait::class.java)
            .orNull()?.getTestCasesFor(AppliesTo.CLIENT).orEmpty().map { TestCase.ResponseTest(it, outputShape) }

        val errorTests = operationIndex.getErrors(operationShape).flatMap { error ->
            val testCases = error.getTrait(HttpResponseTestsTrait::class.java).orNull()?.testCases.orEmpty()
            testCases.map { TestCase.ResponseTest(it, error) }
        }
        val allTests: List<TestCase> = (requestTests + responseTests + errorTests).filterMatching()
        if (allTests.isNotEmpty()) {
            val operationName = operationSymbol.name
            val testModuleName = "${operationName.toSnakeCase()}_request_test"
            val moduleMeta = RustMetadata(
                public = false,
                additionalAttributes = listOf(
                    Attribute.Cfg("test"),
                    Attribute.Custom("allow(unreachable_code, unused_variables)")
                )
            )
            writer.withModule(testModuleName, moduleMeta) {
                renderAllTestCases(allTests)
            }
        }
    }

    private fun RustWriter.renderAllTestCases(allTests: List<TestCase>) {
        allTests.forEach {
            renderTestCaseBlock(it.testCase, this) {
                when (it) {
                    is TestCase.RequestTest -> this.renderHttpRequestTestCase(it.testCase)
                    is TestCase.ResponseTest -> this.renderHttpResponseTestCase(it.testCase, it.targetShape)
                }
            }
        }
    }

    /**
     * Filter out test cases that are disabled or don't match the service protocol
     */
    private fun List<TestCase>.filterMatching(): List<TestCase> {
        return if (RunOnly.isNullOrEmpty()) {
            this.filter { testCase ->
                testCase.testCase.protocol == protocolConfig.protocol &&
                    !DisableTests.contains(testCase.testCase.id)
            }
        } else {
            this.filter { RunOnly.contains(it.testCase.id) }
        }
    }

    private fun renderTestCaseBlock(
        testCase: HttpMessageTestCase,
        testModuleWriter: RustWriter,
        block: RustWriter.() -> Unit
    ) {
        testModuleWriter.setNewlinePrefix("/// ")
        testCase.documentation.map {
            testModuleWriter.writeWithNoFormatting(it)
        }
        testModuleWriter.write("Test ID: ${testCase.id}")
        testModuleWriter.setNewlinePrefix("")
        testModuleWriter.writeWithNoFormatting("#[test]")
        val action = when (testCase) {
            is HttpResponseTestCase -> Action.Response
            is HttpRequestTestCase -> Action.Request
            else -> throw CodegenException("unknown test case type")
        }
        if (expectFail(testCase)) {
            testModuleWriter.writeWithNoFormatting("#[should_panic]")
        }
        val fnName = when (action) {
            is Action.Response -> "_response"
            is Action.Request -> "_request"
        }
        testModuleWriter.rustBlock("fn ${testCase.id.toSnakeCase()}$fnName()") {
            block(this)
        }
    }

    private fun RustWriter.renderHttpRequestTestCase(
        httpRequestTestCase: HttpRequestTestCase
    ) {
        writeInline("let input =")
        instantiator.render(this, inputShape, httpRequestTestCase.params)
        write(";")
        write("let (http_request, _) = input.into_request_response().0.into_parts();")
        with(httpRequestTestCase) {
            write(
                """
                    assert_eq!(http_request.method(), ${method.dq()});
                    assert_eq!(http_request.uri().path(), ${uri.dq()});
                """
            )
            resolvedHost.orNull()?.also { host ->
                rust("""assert_eq!(http_request.uri().host().expect("host should be set"), ${host.dq()});""")
            }
        }
        checkQueryParams(this, httpRequestTestCase.queryParams)
        checkForbidQueryParams(this, httpRequestTestCase.forbidQueryParams)
        checkRequiredQueryParams(this, httpRequestTestCase.requireQueryParams)
        checkHeaders(this, httpRequestTestCase.headers)
        checkForbidHeaders(this, httpRequestTestCase.forbidHeaders)
        checkRequiredHeaders(this, httpRequestTestCase.requireHeaders)
        if (protocolSupport.requestBodySerialization) {
            // "If no request body is defined, then no assertions are made about the body of the message."
            httpRequestTestCase.body.orNull()?.also { body ->
                checkBody(this, body, httpRequestTestCase.bodyMediaType.orNull())
            }
        }

        // Explicitly warn if the test case defined parameters that we aren't doing anything with
        with(httpRequestTestCase) {
            if (authScheme.isPresent) {
                logger.warning("Test case provided authScheme but this was ignored")
            }
            if (!httpRequestTestCase.vendorParams.isEmpty) {
                logger.warning("Test case provided vendorParams but these were ignored")
            }
        }
    }

    private fun HttpMessageTestCase.action(): Action = when (this) {
        is HttpRequestTestCase -> Action.Request
        is HttpResponseTestCase -> Action.Response
        else -> throw CodegenException("Unknown test case type")
    }

    private fun expectFail(testCase: HttpMessageTestCase): Boolean = ExpectFail.find {
        it.id == testCase.id && it.action == testCase.action() && it.service == protocolConfig.serviceShape.id.toString()
    } != null

    private fun RustWriter.renderHttpResponseTestCase(
        testCase: HttpResponseTestCase,
        expectedShape: StructureShape
    ) {
        if (!protocolSupport.responseDeserialization || (
            !protocolSupport.errorDeserialization && expectedShape.hasTrait(
                    ErrorTrait::class.java
                )
            )
        ) {
            rust("/* test case disabled for this protocol (not yet supported) */")
            return
        }
        writeInline("let expected_output =")
        instantiator.render(this, expectedShape, testCase.params)
        write(";")
        write("let http_response = #T::new()", RuntimeType.HttpResponseBuilder)
        testCase.headers.forEach { (key, value) ->
            writeWithNoFormatting(".header(${key.dq()}, ${value.dq()})")
        }
        rust(
            """
                .status(${testCase.code})
                .body(${testCase.body.orNull()?.dq()?.replace("#", "##") ?: "vec![]"})
                .unwrap();
            """
        )
        write("let parsed = #T::from_response(&http_response);", operationSymbol)
        if (expectedShape.hasTrait(ErrorTrait::class.java)) {
            val errorSymbol = operationShape.errorSymbol(protocolConfig.symbolProvider)
            val errorVariant = protocolConfig.symbolProvider.toSymbol(expectedShape).name
            rust("""let parsed = parsed.expect_err("should be error response");""")
            rustBlock("if let #TKind::$errorVariant(actual_error) = parsed.kind", errorSymbol) {
                write("assert_eq!(expected_output, actual_error);")
            }
            rustBlock("else") {
                write("panic!(\"wrong variant: Got: {:?}. Expected: {:?}\", parsed, expected_output);")
            }
        } else {
            write("assert_eq!(parsed.unwrap(), expected_output);")
        }
    }

    private fun checkRequiredHeaders(rustWriter: RustWriter, requireHeaders: List<String>) {
        basicCheck(requireHeaders, rustWriter, "required_headers", "require_headers")
    }

    private fun checkForbidHeaders(rustWriter: RustWriter, forbidHeaders: List<String>) {
        basicCheck(forbidHeaders, rustWriter, "forbidden_headers", "forbid_headers")
    }

    private fun checkBody(rustWriter: RustWriter, body: String, mediaType: String?) {
        rustWriter.write("""let body = http_request.body().bytes().expect("body should be strict");""")
        if (body == "") {
            rustWriter.write("// No body")
            rustWriter.write("assert_eq!(std::str::from_utf8(body).unwrap(), ${"".dq()});")
        } else {
            // When we generate a body instead of a stub, drop the trailing `;` and enable the assertion
            assertOk(rustWriter) {
                rustWriter.write(
                    "#T(&body, ${
                    rustWriter.escape(body).dq()
                    }, #T::from(${(mediaType ?: "unknown").dq()}))",
                    RuntimeType.ProtocolTestHelper(protocolConfig.runtimeConfig, "validate_body"),
                    RuntimeType.ProtocolTestHelper(protocolConfig.runtimeConfig, "MediaType")
                )
            }
        }
    }

    private fun checkHeaders(rustWriter: RustWriter, headers: Map<String, String>) {
        if (headers.isEmpty()) {
            return
        }
        val variableName = "expected_headers"
        rustWriter.withBlock("let $variableName = &[", "];") {
            write(
                headers.entries.joinToString(",") {
                    "(${it.key.dq()}, ${it.value.dq()})"
                }
            )
        }
        assertOk(rustWriter) {
            write(
                "#T(&http_request, $variableName)",
                RuntimeType.ProtocolTestHelper(protocolConfig.runtimeConfig, "validate_headers")
            )
        }
    }

    private fun checkRequiredQueryParams(
        rustWriter: RustWriter,
        requiredParams: List<String>
    ) = basicCheck(requiredParams, rustWriter, "required_params", "require_query_params")

    private fun checkForbidQueryParams(
        rustWriter: RustWriter,
        forbidParams: List<String>
    ) = basicCheck(forbidParams, rustWriter, "forbid_params", "forbid_query_params")

    private fun checkQueryParams(
        rustWriter: RustWriter,
        queryParams: List<String>
    ) = basicCheck(queryParams, rustWriter, "expected_query_params", "validate_query_string")

    private fun basicCheck(
        params: List<String>,
        rustWriter: RustWriter,
        variableName: String,
        checkFunction: String
    ) {
        if (params.isEmpty()) {
            return
        }
        rustWriter.withBlock("let $variableName = ", ";") {
            strSlice(this, params)
        }
        assertOk(rustWriter) {
            write(
                "#T(&http_request, $variableName)",
                RuntimeType.ProtocolTestHelper(protocolConfig.runtimeConfig, checkFunction)
            )
        }
    }

    /**
     * wraps `inner` in a call to `protocol_test_helpers::assert_ok`, a convenience wrapper
     * for pretty prettying protocol test helper results
     */
    private fun assertOk(rustWriter: RustWriter, inner: RustWriter.() -> Unit) {
        rustWriter.write("#T(", RuntimeType.ProtocolTestHelper(protocolConfig.runtimeConfig, "assert_ok"))
        inner(rustWriter)
        rustWriter.write(");")
    }

    private fun strSlice(writer: RustWriter, args: List<String>) {
        writer.withBlock("&[", "]") {
            write(args.joinToString(",") { it.dq() })
        }
    }

    companion object {
        sealed class Action {
            object Request : Action()
            object Response : Action()
        }

        data class FailingTest(val service: String, val id: String, val action: Action)

        // These tests fail due to shortcomings in our implementation.
        // These could be configured via runtime configuration, but since this won't be long-lasting,
        // it makes sense to do the simplest thing for now.
        // The test will _fail_ if these pass, so we will discover & remove if we fix them by accident
        val JsonRpc10 = "aws.protocoltests.json10#JsonRpc10"
        val AwsJson11 = "aws.protocoltests.json#JsonProtocol"
        val RestJson = "aws.protocoltests.restjson#RestJson"
        private val ExpectFail = setOf(
            // Endpoint trait https://github.com/awslabs/smithy-rs/issues/197
            // This will also require running operations through the endpoint middleware (or moving endpoint middleware
            // into operation construction
            FailingTest(JsonRpc10, "AwsJson10EndpointTrait", Action.Request),
            FailingTest(JsonRpc10, "AwsJson10EndpointTraitWithHostLabel", Action.Request),
            FailingTest(AwsJson11, "AwsJson11EndpointTrait", Action.Request),
            FailingTest(AwsJson11, "AwsJson11EndpointTraitWithHostLabel", Action.Request),
            FailingTest(RestJson, "RestJsonEndpointTrait", Action.Request),
            FailingTest(RestJson, "RestJsonEndpointTraitWithHostLabel", Action.Request),
        )
        private val RunOnly: Set<String>? = null

        // These tests are not even attempted to be compiled, either because they will not compile
        // or because they are flaky
        private val DisableTests = setOf(
            // This test is flaky because of set ordering serialization https://github.com/awslabs/smithy-rs/issues/37
            "AwsJson11Enums",
            "RestJsonJsonEnums",
            "RestJsonLists"
        )
    }
}
