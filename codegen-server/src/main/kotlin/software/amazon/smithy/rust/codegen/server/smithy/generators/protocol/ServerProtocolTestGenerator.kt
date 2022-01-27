/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.protocol

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.node.Node
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
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.escape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerHttpProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.testutil.TokioTest
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.orNull
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase
import java.util.logging.Logger

/**
 * Generate protocol tests for an operation
 */
class ServerProtocolTestGenerator(
    private val codegenContext: CodegenContext,
    private val protocolSupport: ProtocolSupport,
    private val operationShape: OperationShape,
    private val writer: RustWriter
) {
    private val logger = Logger.getLogger(javaClass.name)

    private val model = codegenContext.model
    private val inputShape = operationShape.inputShape(codegenContext.model)
    private val outputShape = operationShape.outputShape(codegenContext.model)
    private val symbolProvider = codegenContext.symbolProvider
    private val operationSymbol = symbolProvider.toSymbol(operationShape)
    private val operationIndex = OperationIndex.of(codegenContext.model)
    private val operationImplementationName = "${operationSymbol.name}${ServerHttpProtocolGenerator.OPERATION_OUTPUT_WRAPPER_SUFFIX}"
    private val operationErrorName = "crate::error::${operationSymbol.name}Error"

    private val instantiator = with(codegenContext) {
        Instantiator(symbolProvider, model, runtimeConfig)
    }

    private val codegenScope = arrayOf(
        "Bytes" to RuntimeType.Bytes,
        "SmithyHttp" to CargoDependency.SmithyHttp(codegenContext.runtimeConfig).asType(),
        "Http" to CargoDependency.Http.asType(),
        "Hyper" to CargoDependency.Hyper.asType(),
        "AxumCore" to ServerCargoDependency.AxumCore.asType(),
        "SmithyHttpServer" to CargoDependency.SmithyHttpServer(codegenContext.runtimeConfig).asType(),
        "AssertEq" to CargoDependency.PrettyAssertions.asType().member("assert_eq!")
    )

    sealed class TestCase {
        abstract val testCase: HttpMessageTestCase

        data class RequestTest(override val testCase: HttpRequestTestCase, val targetShape: StructureShape) :
            TestCase()
        data class ResponseTest(override val testCase: HttpResponseTestCase, val targetShape: StructureShape) :
            TestCase()
    }

    fun render() {
        val requestTests = operationShape.getTrait<HttpRequestTestsTrait>()
            ?.getTestCasesFor(AppliesTo.SERVER).orEmpty().map { TestCase.RequestTest(it, inputShape) }
        val responseTests = operationShape.getTrait<HttpResponseTestsTrait>()
            ?.getTestCasesFor(AppliesTo.SERVER).orEmpty().map { TestCase.ResponseTest(it, outputShape) }
        val errorTests = operationIndex.getErrors(operationShape).flatMap { error ->
            val testCases = error.getTrait<HttpResponseTestsTrait>()
                ?.getTestCasesFor(AppliesTo.SERVER).orEmpty()
            testCases.map { TestCase.ResponseTest(it, error) }
        }
        val allTests: List<TestCase> = (requestTests + responseTests + errorTests).filterMatching().fixBroken()

        if (allTests.isNotEmpty()) {
            val operationName = operationSymbol.name
            val testModuleName = "server_${operationName.toSnakeCase()}_test"
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
                    is TestCase.RequestTest -> this.renderHttpRequestTestCase(it.testCase, it.targetShape)
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
                testCase.testCase.protocol == codegenContext.protocol &&
                    !DisableTests.contains(testCase.testCase.id)
            }
        } else {
            this.filter { RunOnly.contains(it.testCase.id) }
        }
    }

    // This function applies a "fix function" to each broken test before we synthesize it.
    // Broken tests are those whose definitions in the `awslabs/smithy` repository are wrong, usually because they have
    // not been written with a server-side perspective in mind.
    private fun List<TestCase>.fixBroken(): List<TestCase> = this.map {
        when (it) {
            is TestCase.RequestTest -> {
                val howToFixIt = BrokenRequestTests[Pair(codegenContext.serviceShape.id.toString(), it.testCase.id)]
                if (howToFixIt == null) {
                    it
                } else {
                    val fixed = howToFixIt(it.testCase)
                    TestCase.RequestTest(fixed, it.targetShape)
                }
            }
            is TestCase.ResponseTest -> {
                val howToFixIt = BrokenResponseTests[Pair(codegenContext.serviceShape.id.toString(), it.testCase.id)]
                if (howToFixIt == null) {
                    it
                } else {
                    val fixed = howToFixIt(it.testCase)
                    TestCase.ResponseTest(fixed, it.targetShape)
                }
            }
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
        TokioTest.render(testModuleWriter)

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
        testModuleWriter.rustBlock("async fn ${testCase.id.toSnakeCase()}$fnName()") {
            block(this)
        }
    }

    private fun RustWriter.renderHttpRequestTestCase(
        httpRequestTestCase: HttpRequestTestCase,
        inputShape: StructureShape,
    ) {
        if (!protocolSupport.requestDeserialization) {
            rust("/* test case disabled for this protocol (not yet supported) */")
            return
        }
        writeInline("let expected =")
        instantiator.render(this, inputShape, httpRequestTestCase.params)
        write(";\n")

        rustTemplate(
            """
                ##[allow(unused_mut)] let mut http_request = http::Request::builder()
                    .uri("${httpRequestTestCase.uri}")
                """,
                *codegenScope
        )
        for (header in httpRequestTestCase.headers) {
            rust(".header(${header.key.dq()}, ${header.value.dq()})")
        }
        rustTemplate(
    """
            .body(#{SmithyHttpServer}::Body::from(#{Bytes}::from_static(b${httpRequestTestCase.body.orNull()?.dq()})))
            .unwrap();
            """,
            *codegenScope
        )
        if (httpRequestTestCase.queryParams.isNotEmpty()) {
            val queryParams = httpRequestTestCase.queryParams.joinToString(separator = "&")
            rust("""*http_request.uri_mut() = "${httpRequestTestCase.uri}?$queryParams".parse().unwrap();""")
        }
        httpRequestTestCase.host.orNull()?.also {
            rust("""todo!("endpoint trait not supported yet");""")
        }
        checkQueryParams(this, httpRequestTestCase.queryParams)
        checkForbidQueryParams(this, httpRequestTestCase.forbidQueryParams)
        checkRequiredQueryParams(this, httpRequestTestCase.requireQueryParams)
        checkHeaders(this, "&http_request.headers()", httpRequestTestCase.headers)
        checkForbidHeaders(this, "&http_request.headers()", httpRequestTestCase.forbidHeaders)
        checkRequiredHeaders(this, "&http_request.headers()", httpRequestTestCase.requireHeaders)
        if (protocolSupport.requestBodyDeserialization) {
            // "If no request body is defined, then no assertions are made about the body of the message."
            httpRequestTestCase.body.orNull()?.also { body ->
                checkBody(this, body, httpRequestTestCase)
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
        it.id == testCase.id && it.action == testCase.action() && it.service == codegenContext.serviceShape.id.toString()
    } != null

    private fun RustWriter.renderHttpResponseTestCase(
        testCase: HttpResponseTestCase,
        expectedShape: StructureShape
    ) {
        if (!protocolSupport.responseSerialization || (
            !protocolSupport.errorSerialization && expectedShape.hasTrait<ErrorTrait>()
            )
        ) {
            rust("/* test case disabled for this protocol (not yet supported) */")
            return
        }
        writeInline("let output =")
        instantiator.render(this, expectedShape, testCase.params)
        write(";")
        val operationImpl = if (operationShape.errors.isNotEmpty()) {
            if (expectedShape.hasTrait<ErrorTrait>()) {
                val variant = symbolProvider.toSymbol(expectedShape).name
                "$operationImplementationName::Error($operationErrorName::$variant(output))"
            } else {
                "$operationImplementationName::Output(output)"
            }
        } else {
            "$operationImplementationName(output)"
        }
        rustTemplate(
            """
            let output = super::$operationImpl;
            use #{AxumCore}::response::IntoResponse;
            let http_response = output.into_response();
            """,
            *codegenScope,
        )
        rustTemplate(
            """
            #{AssertEq}(
                http::StatusCode::from_u16(${testCase.code}).expect("invalid expected HTTP status code"),
                http_response.status()
            );
            """,
            *codegenScope
        )
        checkHeaders(this, "&http_response.headers()", testCase.headers)
        checkForbidHeaders(this, "&http_response.headers()", testCase.forbidHeaders)
        checkRequiredHeaders(this, "&http_response.headers()", testCase.requireHeaders)
        checkHttpExtensions(this)
        if (!testCase.body.isEmpty) {
            rustTemplate(
                """
                let body = #{Hyper}::body::to_bytes(http_response.into_body()).await.expect("unable to extract body to bytes");
                #{AssertEq}(${escape(testCase.body.get()).dq()}, body);
                """,
                *codegenScope
            )
        }
    }

    private fun checkBody(rustWriter: RustWriter, body: String, testCase: HttpRequestTestCase) {
        val operationName = "${operationSymbol.name}${ServerHttpProtocolGenerator.OPERATION_INPUT_WRAPPER_SUFFIX}"
        rustWriter.rustTemplate(
            """
            use #{AxumCore}::extract::FromRequest;
            let mut http_request = #{AxumCore}::extract::RequestParts::new(http_request);
            let input_wrapper = super::$operationName::from_request(&mut http_request).await.expect("failed to parse request");
            let input = input_wrapper.0;
            """,
            *codegenScope,
        )
        if (operationShape.outputShape(model).hasStreamingMember(model)) {
            rustWriter.rust("""todo!("streaming types aren't supported yet");""")
        } else {
            rustWriter.rustTemplate("#{AssertEq}(input, expected);", *codegenScope)
        }
    }

    private fun checkHttpExtensions(rustWriter: RustWriter) {
        rustWriter.rustTemplate(
            """
            let request_extensions = http_response.extensions().get::<aws_smithy_http_server::RequestExtensions>().expect("extension `RequestExtensions` not found");
            #{AssertEq}(request_extensions.namespace, ${operationShape.id.getNamespace().dq()});
            #{AssertEq}(request_extensions.operation_name, ${operationSymbol.name.dq()});
            """.trimIndent(),
            *codegenScope
        )
    }

    private fun checkRequiredHeaders(rustWriter: RustWriter, actualExpression: String, requireHeaders: List<String>) {
        basicCheck(
            requireHeaders,
            rustWriter,
           "required_headers",
           actualExpression,
           "require_headers"
        )
    }

    private fun checkForbidHeaders(rustWriter: RustWriter, actualExpression: String, forbidHeaders: List<String>) {
        basicCheck(
            forbidHeaders,
            rustWriter,
           "forbidden_headers",
            actualExpression,
           "forbid_headers"
        )
    }

    private fun checkHeaders(rustWriter: RustWriter, actualExpression: String, headers: Map<String, String>) {
        if (headers.isEmpty()) {
            return
        }
        val variableName = "expected_headers"
        rustWriter.withBlock("let $variableName = [", "];") {
            writeWithNoFormatting(
                headers.entries.joinToString(",") {
                    "(${it.key.dq()}, ${it.value.dq()})"
                }
            )
        }
        assertOk(rustWriter) {
            write(
                "#T($actualExpression, $variableName)",
                RuntimeType.ProtocolTestHelper(codegenContext.runtimeConfig, "validate_headers")
            )
        }
    }

    private fun checkRequiredQueryParams(
        rustWriter: RustWriter,
        requiredParams: List<String>
    ) = basicCheck(
        requiredParams,
        rustWriter,
       "required_params",
       "&http_request",
       "require_query_params"
    )

    private fun checkForbidQueryParams(
        rustWriter: RustWriter,
        forbidParams: List<String>
    ) = basicCheck(
        forbidParams,
        rustWriter,
       "forbid_params",
       "&http_request",
       "forbid_query_params"
    )

    private fun checkQueryParams(
        rustWriter: RustWriter,
        queryParams: List<String>
    ) = basicCheck(
        queryParams,
        rustWriter,
        "expected_query_params",
        "&http_request",
        "validate_query_string"
    )

    private fun basicCheck(
        params: List<String>,
        rustWriter: RustWriter,
        expectedVariableName: String,
        actualExpression: String,
        checkFunction: String
    ) {
        if (params.isEmpty()) {
            return
        }
        rustWriter.withBlock("let $expectedVariableName = ", ";") {
            strSlice(this, params)
        }
        assertOk(rustWriter) {
            write(
                "#T($actualExpression, $expectedVariableName)",
                RuntimeType.ProtocolTestHelper(codegenContext.runtimeConfig, checkFunction)
            )
        }
    }

    /**
     * wraps `inner` in a call to `aws_smithy_protocol_test::assert_ok`, a convenience wrapper
     * for pretty prettying protocol test helper results
     */
    private fun assertOk(rustWriter: RustWriter, inner: RustWriter.() -> Unit) {
        rustWriter.write("#T(", RuntimeType.ProtocolTestHelper(codegenContext.runtimeConfig, "assert_ok"))
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
        private val JsonRpc10 = "aws.protocoltests.json10#JsonRpc10"
        private val AwsJson11 = "aws.protocoltests.json#JsonProtocol"
        private val RestJson = "aws.protocoltests.restjson#RestJson"
        private val RestXml = "aws.protocoltests.restxml#RestXml"
        private val AwsQuery = "aws.protocoltests.query#AwsQuery"
        private val Ec2Query = "aws.protocoltests.ec2#AwsEc2"
        private val ExpectFail = setOf<FailingTest>(
            // Headers.
            FailingTest(RestJson, "RestJsonHttpWithHeadersButNoPayload", Action.Request),
            FailingTest(RestJson, "RestJsonHttpPrefixHeadersArePresent", Action.Request),
            FailingTest(RestJson, "RestJsonHttpPrefixHeadersAreNotPresent", Action.Request),
            FailingTest(RestJson, "RestJsonSupportsNaNFloatHeaderInputs", Action.Request),
            FailingTest(RestJson, "RestJsonInputAndOutputWithStringHeaders", Action.Response),
            FailingTest(RestJson, "RestJsonHttpPrefixHeadersArePresent", Action.Response),
            FailingTest(RestJson, "RestJsonNullAndEmptyHeaders", Action.Response),
            FailingTest(RestJson, "MediaTypeHeaderOutputBase64", Action.Response),
            FailingTest(RestJson, "RestJsonInputAndOutputWithEnumHeaders", Action.Response),
            FailingTest(RestJson, "RestJsonSupportsNaNFloatHeaderOutputs", Action.Response),
            FailingTest(RestJson, "RestJsonInputAndOutputWithNumericHeaders", Action.Response),
            FailingTest(RestJson, "RestJsonInputAndOutputWithBooleanHeaders", Action.Response),
            FailingTest(RestJson, "RestJsonInputAndOutputWithTimestampHeaders", Action.Response),
            FailingTest(RestJson, "HttpPrefixHeadersResponse", Action.Response),
            FailingTest(RestJson, "RestJsonSupportsNegativeInfinityFloatHeaderOutputs", Action.Response),
            FailingTest(RestJson, "RestJsonSupportsInfinityFloatHeaderOutputs", Action.Response),
            FailingTest(RestJson, "RestJsonInputAndOutputWithQuotedStringHeaders", Action.Response),
            FailingTest(RestJson, "RestJsonTimestampFormatHeaders", Action.Response),

            FailingTest(RestJson, "RestJsonHttpPayloadTraitsWithBlob", Action.Request),
            FailingTest(RestJson, "RestJsonOutputUnionWithUnitMember", Action.Response),
            FailingTest(RestJson, "RestJsonUnitInputAndOutputNoOutput", Action.Response),
            FailingTest(RestJson, "RestJsonQueryStringEscaping", Action.Request),
            FailingTest(RestJson, "RestJsonSupportsNaNFloatQueryValues", Action.Request),
            FailingTest(RestJson, "DocumentOutput", Action.Response),
            FailingTest(RestJson, "DocumentOutputString", Action.Response),
            FailingTest(RestJson, "DocumentOutputNumber", Action.Response),
            FailingTest(RestJson, "DocumentOutputBoolean", Action.Response),
            FailingTest(RestJson, "DocumentOutputArray", Action.Response),
            FailingTest(RestJson, "DocumentTypeAsPayloadOutput", Action.Response),
            FailingTest(RestJson, "DocumentTypeAsPayloadOutputString", Action.Response),
            FailingTest(RestJson, "RestJsonEmptyInputAndEmptyOutput", Action.Response),
            FailingTest(RestJson, "RestJsonEndpointTrait", Action.Request),
            FailingTest(RestJson, "RestJsonEndpointTraitWithHostLabel", Action.Request),
            FailingTest(RestJson, "RestJsonInvalidGreetingError", Action.Response),
            FailingTest(RestJson, "RestJsonComplexErrorWithNoMessage", Action.Response),
            FailingTest(RestJson, "RestJsonFooErrorUsingCode", Action.Response),
            FailingTest(RestJson, "RestJsonFooErrorUsingCodeAndNamespace", Action.Response),
            FailingTest(RestJson, "RestJsonFooErrorUsingCodeUriAndNamespace", Action.Response),
            FailingTest(RestJson, "RestJsonFooErrorWithDunderType", Action.Response),
            FailingTest(RestJson, "RestJsonFooErrorWithDunderTypeAndNamespace", Action.Response),
            FailingTest(RestJson, "RestJsonFooErrorWithDunderTypeUriAndNamespace", Action.Response),
            FailingTest(RestJson, "EnumPayloadResponse", Action.Response),
            FailingTest(RestJson, "RestJsonHttpPayloadTraitsWithBlob", Action.Response),
            FailingTest(RestJson, "RestJsonHttpPayloadTraitsWithNoBlobBody", Action.Response),
            FailingTest(RestJson, "RestJsonHttpPayloadTraitsWithMediaTypeWithBlob", Action.Request),
            FailingTest(RestJson, "RestJsonHttpPayloadTraitsWithMediaTypeWithBlob", Action.Response),
            FailingTest(RestJson, "RestJsonHttpPayloadWithStructure", Action.Request),
            FailingTest(RestJson, "RestJsonHttpPayloadWithStructure", Action.Response),
            FailingTest(RestJson, "RestJsonSupportsNaNFloatLabels", Action.Request),
            FailingTest(RestJson, "RestJsonHttpResponseCode", Action.Response),
            FailingTest(RestJson, "StringPayloadResponse", Action.Response),
            FailingTest(RestJson, "RestJsonIgnoreQueryParamsInResponse", Action.Response),
            FailingTest(RestJson, "RestJsonJsonBlobs", Action.Response),
            FailingTest(RestJson, "RestJsonJsonEnums", Action.Response),
            FailingTest(RestJson, "RestJsonLists", Action.Response),
            FailingTest(RestJson, "RestJsonListsEmpty", Action.Response),
            FailingTest(RestJson, "RestJsonListsSerializeNull", Action.Response),
            FailingTest(RestJson, "RestJsonJsonMaps", Action.Response),
            FailingTest(RestJson, "RestJsonDeserializesNullMapValues", Action.Response),
            FailingTest(RestJson, "RestJsonDeserializesZeroValuesInMaps", Action.Response),
            FailingTest(RestJson, "RestJsonDeserializesSparseSetMap", Action.Response),
            FailingTest(RestJson, "RestJsonDeserializesDenseSetMap", Action.Response),
            FailingTest(RestJson, "RestJsonDeserializesSparseSetMapAndRetainsNull", Action.Response),
            FailingTest(RestJson, "RestJsonJsonTimestamps", Action.Response),
            FailingTest(RestJson, "RestJsonJsonTimestampsWithDateTimeFormat", Action.Response),
            FailingTest(RestJson, "RestJsonJsonTimestampsWithEpochSecondsFormat", Action.Response),
            FailingTest(RestJson, "RestJsonJsonTimestampsWithHttpDateFormat", Action.Response),
            FailingTest(RestJson, "RestJsonDeserializeStringUnionValue", Action.Response),
            FailingTest(RestJson, "RestJsonDeserializeBooleanUnionValue", Action.Response),
            FailingTest(RestJson, "RestJsonDeserializeNumberUnionValue", Action.Response),
            FailingTest(RestJson, "RestJsonDeserializeBlobUnionValue", Action.Response),
            FailingTest(RestJson, "RestJsonDeserializeTimestampUnionValue", Action.Response),
            FailingTest(RestJson, "RestJsonDeserializeEnumUnionValue", Action.Response),
            FailingTest(RestJson, "RestJsonDeserializeListUnionValue", Action.Response),
            FailingTest(RestJson, "RestJsonDeserializeMapUnionValue", Action.Response),
            FailingTest(RestJson, "RestJsonDeserializeStructureUnionValue", Action.Response),
            FailingTest(RestJson, "RestJsonNoInputAndNoOutput", Action.Response),
            FailingTest(RestJson, "RestJsonNoInputAndOutputWithJson", Action.Response),
            FailingTest(RestJson, "RestJsonRecursiveShapes", Action.Response),
            FailingTest(RestJson, "RestJsonSupportsNaNFloatInputs", Action.Request),
            FailingTest(RestJson, "RestJsonSimpleScalarProperties", Action.Response),
            FailingTest(RestJson, "RestJsonServersDontSerializeNullStructureValues", Action.Response),
            FailingTest(RestJson, "RestJsonSupportsNaNFloatInputs", Action.Response),
            FailingTest(RestJson, "RestJsonSupportsInfinityFloatInputs", Action.Response),
            FailingTest(RestJson, "RestJsonSupportsNegativeInfinityFloatInputs", Action.Response),
            FailingTest(RestJson, "RestJsonStreamingTraitsWithBlob", Action.Request),
            FailingTest(RestJson, "RestJsonStreamingTraitsWithNoBlobBody", Action.Request),
            FailingTest(RestJson, "RestJsonStreamingTraitsWithBlob", Action.Response),
            FailingTest(RestJson, "RestJsonStreamingTraitsWithNoBlobBody", Action.Response),
            FailingTest(RestJson, "RestJsonStreamingTraitsRequireLengthWithBlob", Action.Request),
            FailingTest(RestJson, "RestJsonStreamingTraitsRequireLengthWithNoBlobBody", Action.Request),
            FailingTest(RestJson, "RestJsonStreamingTraitsRequireLengthWithBlob", Action.Response),
            FailingTest(RestJson, "RestJsonStreamingTraitsRequireLengthWithNoBlobBody", Action.Response),
            FailingTest(RestJson, "RestJsonStreamingTraitsWithMediaTypeWithBlob", Action.Request),
            FailingTest(RestJson, "RestJsonStreamingTraitsWithMediaTypeWithBlob", Action.Response),
            FailingTest(RestJson, "RestJsonTestBodyStructure", Action.Request),
            FailingTest(RestJson, "RestJsonHttpWithEmptyBody", Action.Request),
            FailingTest(RestJson, "RestJsonHttpWithEmptyBlobPayload", Action.Request),
            FailingTest(RestJson, "RestJsonTestPayloadBlob", Action.Request),
            FailingTest(RestJson, "RestJsonHttpWithEmptyStructurePayload", Action.Request),
            FailingTest(RestJson, "RestJsonTestPayloadStructure", Action.Request),
            FailingTest("com.amazonaws.s3#AmazonS3", "GetBucketLocationUnwrappedOutput", Action.Response),
            FailingTest("com.amazonaws.s3#AmazonS3", "S3DefaultAddressing", Action.Request),
            FailingTest("com.amazonaws.s3#AmazonS3", "S3VirtualHostAddressing", Action.Request),
            FailingTest("com.amazonaws.s3#AmazonS3", "S3PathAddressing", Action.Request),
            FailingTest("com.amazonaws.s3#AmazonS3", "S3VirtualHostDualstackAddressing", Action.Request),
            FailingTest("com.amazonaws.s3#AmazonS3", "S3VirtualHostAccelerateAddressing", Action.Request),
            FailingTest("com.amazonaws.s3#AmazonS3", "S3VirtualHostDualstackAccelerateAddressing", Action.Request),
            FailingTest("com.amazonaws.s3#AmazonS3", "S3OperationAddressingPreferred", Action.Request),
        )
        private val RunOnly: Set<String>? = null

        // These tests are not even attempted to be generated, either because they will not compile
        // or because they are flaky
        private val DisableTests = setOf<String>()

        private fun fixRestJsonSupportsNaNFloatQueryValues(testCase: HttpRequestTestCase): HttpRequestTestCase {
            // TODO This test does not pass, even after fixing it with this function, because, in IEEE 754 floating
            // point numbers, `NaN` is not equal to any other floating point number, even itself! So we can't compare it
            // to any "expected" value.
            // Reference: https://doc.rust-lang.org/std/primitive.f32.html
            // Request for guidance about this test to Smithy team: https://github.com/awslabs/smithy/pull/1040#discussion_r780418707
            val params = Node.parse(
                """
                {
                    "queryFloat": "NaN",
                    "queryDouble": "NaN",
                    "queryParamsMapOfStringList": {
                        "Float": ["NaN"],
                        "Double": ["NaN"]
                    }
                }
                """.trimIndent()
            ).asObjectNode().get()

            return testCase.toBuilder().params(params).build()
        }
        private fun fixRestJsonSupportsInfinityFloatQueryValues(testCase: HttpRequestTestCase): HttpRequestTestCase =
            testCase.toBuilder().params(
                Node.parse(
                    """
                    {
                        "queryFloat": "Infinity",
                        "queryDouble": "Infinity",
                        "queryParamsMapOfStringList": {
                            "Float": ["Infinity"],
                            "Double": ["Infinity"]
                        }
                    }
                    """.trimMargin()
                ).asObjectNode().get()
            ).build()
        private fun fixRestJsonSupportsNegativeInfinityFloatQueryValues(testCase: HttpRequestTestCase): HttpRequestTestCase =
            testCase.toBuilder().params(
                Node.parse(
                    """
                    {
                        "queryFloat": "-Infinity",
                        "queryDouble": "-Infinity",
                        "queryParamsMapOfStringList": {
                            "Float": ["-Infinity"],
                            "Double": ["-Infinity"]
                        }
                    }
                    """.trimMargin()
                ).asObjectNode().get()
            ).build()
        private fun fixRestJsonAllQueryStringTypes(testCase: HttpRequestTestCase): HttpRequestTestCase =
             testCase.toBuilder().params(
                 Node.parse("""{
                    "queryString": "Hello there",
                    "queryStringList": ["a", "b", "c"],
                    "queryStringSet": ["a", "b", "c"],
                    "queryByte": 1,
                    "queryShort": 2,
                    "queryInteger": 3,
                    "queryIntegerList": [1, 2, 3],
                    "queryIntegerSet": [1, 2, 3],
                    "queryLong": 4,
                    "queryFloat": 1.1,
                    "queryDouble": 1.1,
                    "queryDoubleList": [1.1, 2.1, 3.1],
                    "queryBoolean": true,
                    "queryBooleanList": [true, false, true],
                    "queryTimestamp": 1,
                    "queryTimestampList": [1, 2, 3],
                    "queryEnum": "Foo",
                    "queryEnumList": ["Foo", "Baz", "Bar"],
                    "queryParamsMapOfStringList": {
                        "String": ["Hello there"],
                        "StringList": ["a", "b", "c"],
                        "StringSet": ["a", "b", "c"],
                        "Byte": ["1"],
                        "Short": ["2"],
                        "Integer": ["3"],
                        "IntegerList": ["1", "2", "3"],
                        "IntegerSet": ["1", "2", "3"],
                        "Long": ["4"],
                        "Float": ["1.1"],
                        "Double": ["1.1"],
                        "DoubleList": ["1.1", "2.1", "3.1"],
                        "Boolean": ["true"],
                        "BooleanList": ["true", "false", "true"],
                        "Timestamp": ["1970-01-01T00:00:01Z"],
                        "TimestampList": ["1970-01-01T00:00:01Z", "1970-01-01T00:00:02Z", "1970-01-01T00:00:03Z"],
                        "Enum": ["Foo"],
                        "EnumList": ["Foo", "Baz", "Bar"]
                    }
                }""".trimMargin()).asObjectNode().get()
             ).build()
        // This test assumes that errors in responses are identified by an `X-Amzn-Errortype` header with the error shape name.
        // However, Smithy specifications for AWS protocols that serialize to JSON recommend that new server implementations
        // serialize error types using a `__type` field in the body.
        // Our implementation follows this recommendation, so we fix the test by removing the header and instead expecting
        // the error type to be in the body.
        private fun fixRestJsonEmptyComplexErrorWithNoMessage(testCase: HttpResponseTestCase): HttpResponseTestCase =
            testCase.toBuilder()
                .headers(emptyMap())
                .body("""{"__type":"ComplexError"}""")
                .build()

        // These are tests whose definitions in the `awslabs/smithy` repository are wrong.
        // This is because they have not been written from a server perspective, and as such the expected `params` field is incomplete.
        // TODO Contribute a PR to fix them upstream and remove them from this list once the fixes get published in the next Smithy release.
        private val BrokenRequestTests = mapOf(
            // https://github.com/awslabs/smithy/pull/1040
            Pair(RestJson, "RestJsonSupportsNaNFloatQueryValues") to ::fixRestJsonSupportsNaNFloatQueryValues,
            Pair(RestJson, "RestJsonSupportsInfinityFloatQueryValues") to ::fixRestJsonSupportsInfinityFloatQueryValues,
            Pair(RestJson, "RestJsonSupportsNegativeInfinityFloatQueryValues") to ::fixRestJsonSupportsNegativeInfinityFloatQueryValues,
            Pair(RestJson, "RestJsonAllQueryStringTypes") to ::fixRestJsonAllQueryStringTypes
        )

        private val BrokenResponseTests = mapOf(
            Pair(RestJson, "RestJsonEmptyComplexErrorWithNoMessage") to ::fixRestJsonEmptyComplexErrorWithNoMessage
        )
    }
}
