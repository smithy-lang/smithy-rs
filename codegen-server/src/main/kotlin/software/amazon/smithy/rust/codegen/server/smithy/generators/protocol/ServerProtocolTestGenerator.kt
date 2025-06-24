/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.protocol

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.protocoltests.traits.AppliesTo
import software.amazon.smithy.protocoltests.traits.HttpMalformedRequestTestCase
import software.amazon.smithy.protocoltests.traits.HttpMalformedResponseBodyDefinition
import software.amazon.smithy.protocoltests.traits.HttpMalformedResponseDefinition
import software.amazon.smithy.protocoltests.traits.HttpRequestTestCase
import software.amazon.smithy.protocoltests.traits.HttpResponseTestCase
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.BrokenTest
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.FailingTest
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolTestGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ServiceShapeId.AWS_JSON_10
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ServiceShapeId.AWS_JSON_11
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ServiceShapeId.REST_JSON
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ServiceShapeId.REST_JSON_VALIDATION
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ServiceShapeId.RPC_V2_CBOR
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ServiceShapeId.RPC_V2_CBOR_EXTRAS
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.TestCase
import software.amazon.smithy.rust.codegen.core.smithy.transformers.allErrors
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerInstantiator
import java.util.logging.Logger

/**
 * Generate server protocol tests for an [operationShape].
 */
class ServerProtocolTestGenerator(
    override val codegenContext: CodegenContext,
    override val protocolSupport: ProtocolSupport,
    override val operationShape: OperationShape,
) : ProtocolTestGenerator() {
    companion object {
        private val ExpectFail: Set<FailingTest> =
            setOf(
                // Endpoint trait is not implemented yet, see https://github.com/smithy-lang/smithy-rs/issues/950.
                FailingTest.RequestTest(REST_JSON, "RestJsonEndpointTrait"),
                FailingTest.RequestTest(REST_JSON, "RestJsonEndpointTraitWithHostLabel"),
                FailingTest.RequestTest(REST_JSON, "RestJsonOmitsEmptyListQueryValues"),
                FailingTest.ResponseTest(REST_JSON, "RestJsonNullAndEmptyHeaders"),
                FailingTest.ResponseTest(REST_JSON, "RestJsonHttpPayloadWithStructureAndEmptyResponseBody"),
                // Tests involving `@range` on floats.
                // Pending resolution from the Smithy team, see https://github.com/smithy-lang/smithy-rs/issues/2007.
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedRangeFloat_case0"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedRangeFloat_case1"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedRangeMaxFloat"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedRangeMinFloat"),
                // Tests involving floating point shapes and the `@range` trait; see https://github.com/smithy-lang/smithy-rs/issues/2007
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedRangeFloatOverride_case0"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedRangeFloatOverride_case1"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedRangeMaxFloatOverride"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedRangeMinFloatOverride"),
                // Some tests for the S3 service (restXml).
                FailingTest.ResponseTest("com.amazonaws.s3#AmazonS3", "GetBucketLocationUnwrappedOutput"),
                FailingTest.RequestTest("com.amazonaws.s3#AmazonS3", "S3DefaultAddressing"),
                FailingTest.RequestTest("com.amazonaws.s3#AmazonS3", "S3VirtualHostAddressing"),
                FailingTest.RequestTest("com.amazonaws.s3#AmazonS3", "S3PathAddressing"),
                FailingTest.RequestTest("com.amazonaws.s3#AmazonS3", "S3VirtualHostDualstackAddressing"),
                FailingTest.RequestTest("com.amazonaws.s3#AmazonS3", "S3VirtualHostAccelerateAddressing"),
                FailingTest.RequestTest("com.amazonaws.s3#AmazonS3", "S3VirtualHostDualstackAccelerateAddressing"),
                FailingTest.RequestTest("com.amazonaws.s3#AmazonS3", "S3OperationAddressingPreferred"),
                FailingTest.ResponseTest("com.amazonaws.s3#AmazonS3", "S3OperationNoErrorWrappingResponse"),
                // AwsJson1.0 failing tests.
                FailingTest.RequestTest("aws.protocoltests.json10#JsonRpc10", "AwsJson10EndpointTraitWithHostLabel"),
                FailingTest.RequestTest("aws.protocoltests.json10#JsonRpc10", "AwsJson10EndpointTrait"),
                // AwsJson1.1 failing tests.
                FailingTest.RequestTest(AWS_JSON_11, "AwsJson11EndpointTraitWithHostLabel"),
                FailingTest.RequestTest(AWS_JSON_11, "AwsJson11EndpointTrait"),
                FailingTest.ResponseTest(AWS_JSON_11, "parses_the_request_id_from_the_response"),
                // TODO(https://github.com/awslabs/smithy/issues/1683): This has been marked as failing until resolution of said issue
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedUniqueItemsBlobList"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedUniqueItemsBooleanList_case0"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedUniqueItemsBooleanList_case1"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedUniqueItemsStringList"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedUniqueItemsByteList"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedUniqueItemsShortList"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedUniqueItemsIntegerList"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedUniqueItemsLongList"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedUniqueItemsTimestampList"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedUniqueItemsDateTimeList"),
                FailingTest.MalformedRequestTest(
                    REST_JSON_VALIDATION,
                    "RestJsonMalformedUniqueItemsHttpDateList_case0",
                ),
                FailingTest.MalformedRequestTest(
                    REST_JSON_VALIDATION,
                    "RestJsonMalformedUniqueItemsHttpDateList_case1",
                ),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedUniqueItemsEnumList"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedUniqueItemsIntEnumList"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedUniqueItemsListList"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedUniqueItemsStructureList"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedUniqueItemsUnionList_case0"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedUniqueItemsUnionList_case1"),
                // TODO(https://github.com/smithy-lang/smithy-rs/issues/2472): We don't respect the `@internal` trait
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedEnumList_case0"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedEnumList_case1"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedEnumMapKey_case0"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedEnumMapKey_case1"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedEnumMapValue_case0"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedEnumMapValue_case1"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedEnumString_case0"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedEnumString_case1"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedEnumUnion_case0"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedEnumUnion_case1"),
                // TODO(https://github.com/awslabs/smithy/issues/1737): Specs on @internal, @tags, and enum values need to be clarified
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedEnumTraitString_case0"),
                FailingTest.MalformedRequestTest(REST_JSON_VALIDATION, "RestJsonMalformedEnumTraitString_case1"),
                FailingTest.MalformedRequestTest(
                    REST_JSON,
                    "RestJsonWithoutBodyEmptyInputExpectsEmptyContentType",
                ),
                // These tests are broken because they are missing a target header.
                FailingTest.RequestTest(AWS_JSON_10, "AwsJson10ServerPopulatesNestedDefaultsWhenMissingInRequestBody"),
                FailingTest.RequestTest(AWS_JSON_10, "AwsJson10ServerPopulatesDefaultsWhenMissingInRequestBody"),
                FailingTest.ResponseTest(AWS_JSON_11, "AwsJson11IntEnums"),
                // For the following 4 tests, response defaults are not set when builders are not used
                // https://github.com/smithy-lang/smithy-rs/issues/3339
                FailingTest.ResponseTest(AWS_JSON_10, "AwsJson10ServerPopulatesDefaultsInResponseWhenMissingInParams"),
                FailingTest.ResponseTest(
                    AWS_JSON_10,
                    "AwsJson10ServerPopulatesNestedDefaultValuesWhenMissingInInResponseParams",
                ),
                // TODO(https://github.com/smithy-lang/smithy-rs/issues/3723): This affects all protocols
                FailingTest.MalformedRequestTest(RPC_V2_CBOR_EXTRAS, "AdditionalTokensEmptyStruct"),
                // TODO(https://github.com/smithy-lang/smithy-rs/issues/3339)
                FailingTest.ResponseTest(RPC_V2_CBOR, "RpcV2CborServerPopulatesDefaultsInResponseWhenMissingInParams"),
                FailingTest.ResponseTest(REST_JSON, "RestJsonServerPopulatesDefaultsInResponseWhenMissingInParams"),
                FailingTest.ResponseTest(
                    REST_JSON,
                    "RestJsonServerPopulatesNestedDefaultValuesWhenMissingInInResponseParams",
                ),
                // TODO(https://github.com/smithy-lang/smithy-rs/issues/3743): We need to be able to configure
                //  instantiator so that it uses default _modeled_ values; `""` is not a valid enum value for `defaultEnum`.
                FailingTest.RequestTest(RPC_V2_CBOR, "RpcV2CborServerPopulatesDefaultsWhenMissingInRequestBody"),
                // TODO(https://github.com/smithy-lang/smithy-rs/issues/3735): Null `Document` may come through a request even though its shape is `@required`
                FailingTest.RequestTest(REST_JSON, "RestJsonServerPopulatesDefaultsWhenMissingInRequestBody"),
                // TODO(https://github.com/smithy-lang/smithy-rs/issues/4184): Issue in both Client and Server with httpPrefixHeaders
                FailingTest.ResponseTest(REST_JSON, "RestJsonHttpEmptyPrefixHeadersResponseServer"),
            )

        private val BrokenTests:
            Set<BrokenTest> =
            setOf(
                BrokenTest.MalformedRequestTest(
                    REST_JSON_VALIDATION,
                    "RestJsonMalformedPatternReDOSString",
                    howToFixItFn = ::fixRestJsonMalformedPatternReDOSString,
                    inAtLeast = setOf("1.26.2", "1.49.0"),
                    trackedIn =
                        setOf(
                            // TODO(https://github.com/awslabs/smithy/issues/1506)
                            "https://github.com/awslabs/smithy/issues/1506",
                            // TODO(https://github.com/smithy-lang/smithy/pull/2340)
                            "https://github.com/smithy-lang/smithy/pull/2340",
                        ),
                ),
            )

        private val DisabledTests =
            setOf<String>(
                // TODO(https://github.com/smithy-lang/smithy-rs/issues/2891): Implement support for `@requestCompression`
                "SDKAppendedGzipAfterProvidedEncoding_restJson1",
                "SDKAppendedGzipAfterProvidedEncoding_restXml",
                "SDKAppendsGzipAndIgnoresHttpProvidedEncoding_awsJson1_0",
                "SDKAppendsGzipAndIgnoresHttpProvidedEncoding_awsJson1_1",
                "SDKAppendsGzipAndIgnoresHttpProvidedEncoding_awsQuery",
                "SDKAppendsGzipAndIgnoresHttpProvidedEncoding_ec2Query",
                "SDKAppliedContentEncoding_awsJson1_0",
                "SDKAppliedContentEncoding_awsJson1_1",
                "SDKAppliedContentEncoding_awsQuery",
                "SDKAppliedContentEncoding_ec2Query",
                "SDKAppliedContentEncoding_restJson1",
                "SDKAppliedContentEncoding_restXml",
                // RestXml S3 tests that fail to compile
                "S3EscapeObjectKeyInUriLabel",
                "S3EscapePathObjectKeyInUriLabel",
                "S3PreservesLeadingDotSegmentInUriLabel",
                "S3PreservesEmbeddedDotSegmentInUriLabel",
            )

        private fun fixRestJsonMalformedPatternReDOSString(
            testCase: TestCase.MalformedRequestTest,
        ): TestCase.MalformedRequestTest {
            val brokenResponse = testCase.testCase.response
            val brokenBody = brokenResponse.body.get()
            val fixedBody =
                HttpMalformedResponseBodyDefinition.builder()
                    .mediaType(brokenBody.mediaType)
                    .contents(
                        """
                        {
                            "message" : "1 validation error detected. Value at '/evilString' failed to satisfy constraint: Member must satisfy regular expression pattern: ^([0-9]+)+${'$'}",
                            "fieldList" : [{"message": "Value at '/evilString' failed to satisfy constraint: Member must satisfy regular expression pattern: ^([0-9]+)+${'$'}", "path": "/evilString"}]
                        }
                        """.trimIndent(),
                    )
                    .build()

            return TestCase.MalformedRequestTest(
                testCase.testCase.toBuilder()
                    .response(brokenResponse.toBuilder().body(fixedBody).build())
                    .build(),
            )
        }
    }

    override val appliesTo: AppliesTo
        get() = AppliesTo.SERVER
    override val expectFail: Set<FailingTest>
        get() = ExpectFail
    override val brokenTests: Set<BrokenTest>
        get() = BrokenTests
    override val generateOnly: Set<String>
        get() = emptySet()
    override val disabledTests: Set<String>
        get() = DisabledTests

    override val logger: Logger = Logger.getLogger(javaClass.name)

    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val operationSymbol = symbolProvider.toSymbol(operationShape)

    private val serviceName = codegenContext.serviceShape.id.name.toPascalCase()
    private val operations =
        TopDownIndex.of(codegenContext.model).getContainedOperations(codegenContext.serviceShape).sortedBy { it.id }

    private val operationInputOutputTypes =
        operations.associateWith {
            val inputSymbol = symbolProvider.toSymbol(it.inputShape(model))
            val outputSymbol = symbolProvider.toSymbol(it.outputShape(model))
            val operationSymbol = symbolProvider.toSymbol(it)

            val inputT = inputSymbol.fullName
            val t = outputSymbol.fullName
            val outputT =
                if (it.errors.isEmpty()) {
                    t
                } else {
                    val errorType = RuntimeType("crate::error::${operationSymbol.name}Error")
                    val e = errorType.fullyQualifiedName()
                    "Result<$t, $e>"
                }

            inputT to outputT
        }

    private val instantiator = ServerInstantiator(codegenContext, withinTest = true)

    private val codegenScope =
        arrayOf(
            "AssertEq" to RuntimeType.PrettyAssertions.resolve("assert_eq!"),
            "Base64SimdDev" to ServerCargoDependency.Base64SimdDev.toType(),
            "Bytes" to RuntimeType.Bytes,
            "Hyper" to RuntimeType.Hyper,
            "MediaType" to RuntimeType.protocolTest(codegenContext.runtimeConfig, "MediaType"),
            "Tokio" to ServerCargoDependency.TokioDev.toType(),
            "Tower" to RuntimeType.Tower,
            "SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
            "decode_body_data" to RuntimeType.protocolTest(codegenContext.runtimeConfig, "decode_body_data"),
        )

    override fun RustWriter.renderAllTestCases(allTests: List<TestCase>) {
        for (it in allTests) {
            renderTestCaseBlock(it, this) {
                when (it) {
                    is TestCase.RequestTest -> this.renderHttpRequestTestCase(it.testCase)
                    is TestCase.ResponseTest -> this.renderHttpResponseTestCase(it.testCase, it.targetShape)
                    is TestCase.MalformedRequestTest -> this.renderHttpMalformedRequestTestCase(it.testCase)
                }
            }
        }
    }

    /**
     * Renders an HTTP request test case.
     * We are given an HTTP request in the test case, and we assert that when we deserialize said HTTP request into
     * an operation's input shape, the resulting shape is of the form we expect, as defined in the test case.
     */
    private fun RustWriter.renderHttpRequestTestCase(httpRequestTestCase: HttpRequestTestCase) {
        logger.info("Generating request test: ${httpRequestTestCase.id}")

        if (!protocolSupport.requestDeserialization) {
            rust("/* test case disabled for this protocol (not yet supported) */")
            return
        }

        with(httpRequestTestCase) {
            renderHttpRequest(
                uri,
                method,
                headers,
                body.orNull(),
                bodyMediaType.orNull(),
                queryParams,
                host.orNull(),
            )
        }
        if (protocolSupport.requestBodyDeserialization) {
            makeRequest(operationShape, operationSymbol, this, checkRequestHandler(operationShape, httpRequestTestCase))
            checkHandlerWasEntered(this)
        }

        // Explicitly warn if the test case defined parameters that we aren't doing anything with.
        with(httpRequestTestCase) {
            if (authScheme.isPresent) {
                logger.warning("Test case provided authScheme but this was ignored")
            }
            if (!httpRequestTestCase.vendorParams.isEmpty) {
                logger.warning("Test case provided vendorParams but these were ignored")
            }
        }
    }

    /**
     * Renders an HTTP response test case.
     * We are given an operation output shape or an error shape in the `params` field, and we assert that when we
     * serialize said shape, the resulting HTTP response is of the form we expect, as defined in the test case.
     * [shape] is either an operation output shape or an error shape.
     */
    private fun RustWriter.renderHttpResponseTestCase(
        testCase: HttpResponseTestCase,
        shape: StructureShape,
    ) {
        logger.info("Generating response test: ${testCase.id}")

        val operationErrorName = "crate::error::${operationSymbol.name}Error"

        if (!protocolSupport.responseSerialization || (
                !protocolSupport.errorSerialization && shape.hasTrait<ErrorTrait>()
            )
        ) {
            rust("/* test case disabled for this protocol (not yet supported) */")
            return
        }
        writeInline("let output =")
        instantiator.render(this, shape, testCase.params)
        rust(";")
        if (operationShape.allErrors(model).isNotEmpty() && shape.hasTrait<ErrorTrait>()) {
            val variant = symbolProvider.toSymbol(shape).name
            rust("let output = $operationErrorName::$variant(output);")
        }
        rustTemplate(
            """
            use #{SmithyHttpServer}::response::IntoResponse;
            let http_response = output.into_response();
            """,
            *codegenScope,
        )
        checkResponse(this, testCase)
    }

    /**
     * Renders an HTTP malformed request test case.
     * We are given a request definition and a response definition, and we have to assert that the request is rejected
     * with the given response.
     */
    private fun RustWriter.renderHttpMalformedRequestTestCase(testCase: HttpMalformedRequestTestCase) {
        logger.info("Generating malformed request test: ${testCase.id}")

        val (_, outputT) = operationInputOutputTypes[operationShape]!!

        val panicMessage = "request should have been rejected, but we accepted it; we parsed operation input `{:?}`"

        rustBlock("") {
            with(testCase.request) {
                // TODO(https://github.com/awslabs/smithy/issues/1102): `uri` should probably not be an `Optional`.
                renderHttpRequest(
                    uri.get(),
                    method,
                    headers,
                    body.orNull(),
                    bodyMediaType.orNull(),
                    queryParams,
                    host.orNull(),
                )
            }

            makeRequest(
                operationShape,
                operationSymbol,
                this,
                writable("""panic!("$panicMessage", &input) as $outputT"""),
            )
            checkResponse(this, testCase.response)
        }
    }

    private fun RustWriter.renderHttpRequest(
        uri: String,
        method: String,
        headers: Map<String, String>,
        body: String?,
        bodyMediaType: String?,
        queryParams: List<String>,
        host: String?,
    ) {
        rustTemplate(
            """
            ##[allow(unused_mut)]
            let mut http_request = http::Request::builder()
                .uri("$uri")
                .method("$method")
            """,
            *codegenScope,
        )
        for (header in headers) {
            rust(".header(${header.key.dq()}, ${header.value.dq()})")
        }
        rustTemplate(
            """
            .body(${
                if (body != null) {
                    // The `replace` is necessary to fix the malformed request test `RestJsonInvalidJsonBody`.
                    // https://github.com/awslabs/smithy/blob/887ae4f6d118e55937105583a07deb90d8fabe1c/smithy-aws-protocol-tests/model/restJson1/malformedRequests/malformed-request-body.smithy#L47
                    //
                    // Smithy is written in Java, which parses `\u000c` within a `String` as a single char given by the
                    // corresponding Unicode code point. That is the "form feed" 0x0c character. When printing it,
                    // it gets written as "\f", which is an invalid Rust escape sequence: https://static.rust-lang.org/doc/master/reference.html#literals
                    // So we need to write the corresponding Rust Unicode escape sequence to make the program compile.
                    //
                    // We also escape to avoid interactions with templating in the case where the body contains `#`.
                    val sanitizedBody = escape(body.replace("\u000c", "\\u{000c}")).dq()

                    val encodedBody = """
                        #{Bytes}::copy_from_slice(
                            &#{decode_body_data}($sanitizedBody.as_bytes(), #{MediaType}::from(${(bodyMediaType ?: "unknown").dq()}))
                        )
                        """

                    "#{SmithyHttpServer}::body::Body::from($encodedBody)"
                } else {
                    "#{SmithyHttpServer}::body::Body::empty()"
                }
            }).unwrap();
                        """,
            *codegenScope,
        )
        if (queryParams.isNotEmpty()) {
            val queryParamsString = queryParams.joinToString(separator = "&")
            rust("""*http_request.uri_mut() = "$uri?$queryParamsString".parse().unwrap();""")
        }
        if (host != null) {
            rust("""todo!("endpoint trait not supported yet");""")
        }
    }

    /** Returns the body of the operation handler in a request test. */
    private fun checkRequestHandler(
        operationShape: OperationShape,
        httpRequestTestCase: HttpRequestTestCase,
    ) = writable {
        val inputShape = operationShape.inputShape(codegenContext.model)
        val outputShape = operationShape.outputShape(codegenContext.model)

        // Construct expected operation input.
        withBlock("let expected = ", ";") {
            instantiator.render(this, inputShape, httpRequestTestCase.params, httpRequestTestCase.headers)
        }

        checkRequestParams(inputShape, this)

        // Construct a dummy response.
        withBlock("let output = ", ";") {
            instantiator.render(this, outputShape, Node.objectNode())
        }

        if (operationShape.errors.isEmpty()) {
            rust("output")
        } else {
            rust("Ok(output)")
        }
    }

    /** Checks the request. */
    private fun makeRequest(
        operationShape: OperationShape,
        operationSymbol: Symbol,
        rustWriter: RustWriter,
        body: Writable,
    ) {
        val (inputT, _) = operationInputOutputTypes[operationShape]!!
        val operationName = RustReservedWords.escapeIfNeeded(operationSymbol.name.toSnakeCase())
        rustWriter.rustTemplate(
            """
            ##[allow(unused_mut)]
            let (sender, mut receiver) = #{Tokio}::sync::mpsc::channel(1);
            let config = crate::service::${serviceName}Config::builder().build();
            let service = crate::service::$serviceName::builder::<#{Hyper}::body::Body, _, _, _>(config)
                .$operationName(move |input: $inputT| {
                    let sender = sender.clone();
                    async move {
                        let result = { #{Body:W} };
                        sender.send(()).await.expect("receiver dropped early");
                        result
                    }
                })
                .build_unchecked();
            let http_response = #{Tower}::ServiceExt::oneshot(service, http_request)
                .await
                .expect("unable to make an HTTP request");
            """,
            "Body" to body,
            *codegenScope,
        )
    }

    private fun checkHandlerWasEntered(rustWriter: RustWriter) {
        rustWriter.rust(
            """
            assert!(receiver.recv().await.is_some(), "we expected operation handler to be invoked but it was not entered");
            """,
        )
    }

    private fun checkRequestParams(
        inputShape: StructureShape,
        rustWriter: RustWriter,
    ) {
        if (inputShape.hasStreamingMember(model)) {
            // A streaming shape does not implement `PartialEq`, so we have to iterate over the input shape's members
            // and handle the equality assertion separately.
            for (member in inputShape.members()) {
                val memberName = codegenContext.symbolProvider.toMemberName(member)
                if (member.isStreaming(codegenContext.model)) {
                    rustWriter.rustTemplate(
                        """
                        #{AssertEq}(
                            input.$memberName.collect().await.unwrap().into_bytes(),
                            expected.$memberName.collect().await.unwrap().into_bytes()
                        );
                        """,
                        *codegenScope,
                    )
                } else {
                    rustWriter.rustTemplate(
                        """
                        #{AssertEq}(input.$memberName, expected.$memberName, "Unexpected value for `$memberName`");
                        """,
                        *codegenScope,
                    )
                }
            }
        } else {
            val hasFloatingPointMembers =
                inputShape.members().any {
                    val target = model.expectShape(it.target)
                    (target is DoubleShape) || (target is FloatShape)
                }

            // TODO(https://github.com/smithy-lang/smithy-rs/issues/1147) Handle the case of nested floating point members.
            if (hasFloatingPointMembers) {
                for (member in inputShape.members()) {
                    val memberName = codegenContext.symbolProvider.toMemberName(member)
                    when (codegenContext.model.expectShape(member.target)) {
                        is DoubleShape, is FloatShape -> {
                            rustWriter.addUseImports(
                                RuntimeType.protocolTest(codegenContext.runtimeConfig, "FloatEquals")
                                    .toSymbol(),
                            )
                            rustWriter.rust(
                                """
                                assert!(input.$memberName.float_equals(&expected.$memberName),
                                    "Unexpected value for `$memberName` {:?} vs. {:?}", expected.$memberName, input.$memberName);
                                """,
                            )
                        }

                        else -> {
                            rustWriter.rustTemplate(
                                """
                                #{AssertEq}(input.$memberName, expected.$memberName, "Unexpected value for `$memberName`");
                                """,
                                *codegenScope,
                            )
                        }
                    }
                }
            } else {
                rustWriter.rustTemplate("#{AssertEq}(input, expected);", *codegenScope)
            }
        }
    }

    private fun checkResponse(
        rustWriter: RustWriter,
        testCase: HttpResponseTestCase,
    ) {
        checkStatusCode(rustWriter, testCase.code)
        checkHeaders(rustWriter, "http_response.headers()", testCase.headers)
        checkForbidHeaders(rustWriter, "http_response.headers()", testCase.forbidHeaders)
        checkRequiredHeaders(rustWriter, "http_response.headers()", testCase.requireHeaders)

        // We can't check that the `OperationExtension` is set in the response, because it is set in the implementation
        // of the operation `Handler` trait, a code path that does not get exercised when we don't have a request to
        // invoke it with (like in the case of an `httpResponseTest` test case).
        // In https://github.com/smithy-lang/smithy-rs/pull/1708: We did change `httpResponseTest`s generation to `call()`
        // the operation handler trait implementation instead of directly calling `from_request()`.

        // If no request body is defined, then no assertions are made about the body of the message.
        if (testCase.body.isPresent) {
            checkBody(rustWriter, testCase.body.get(), testCase.bodyMediaType.orNull())
        }
    }

    private fun checkResponse(
        rustWriter: RustWriter,
        testCase: HttpMalformedResponseDefinition,
    ) {
        checkStatusCode(rustWriter, testCase.code)
        checkHeaders(rustWriter, "http_response.headers()", testCase.headers)

        // We can't check that the `OperationExtension` is set in the response, because it is set in the implementation
        // of the operation `Handler` trait, a code path that does not get exercised when we don't have a request to
        // invoke it with (like in the case of an `httpResponseTest` test case).
        // In https://github.com/smithy-lang/smithy-rs/pull/1708: We did change `httpResponseTest`s generation to `call()`
        // the operation handler trait implementation instead of directly calling `from_request()`.

        // If no request body is defined, then no assertions are made about the body of the message.
        if (testCase.body.isEmpty) return

        val httpMalformedResponseBodyDefinition = testCase.body.get()
        // From https://smithy.io/2.0/additional-specs/http-protocol-compliance-tests.html#httpmalformedresponsebodyassertion
        //
        //     A union describing the assertion to run against the response body. As it is a union, exactly one
        //     member must be set.
        //
        if (httpMalformedResponseBodyDefinition.contents.isPresent) {
            checkBody(
                rustWriter,
                httpMalformedResponseBodyDefinition.contents.get(),
                httpMalformedResponseBodyDefinition.mediaType,
            )
        } else {
            check(httpMalformedResponseBodyDefinition.messageRegex.isPresent)
            // There aren't any restJson1 protocol tests that make use of `messageRegex`.
            TODO("`messageRegex` handling not yet implemented")
        }
    }

    private fun checkBody(
        rustWriter: RustWriter,
        body: String,
        mediaType: String?,
    ) {
        rustWriter.rustTemplate(
            """
            let body = #{Hyper}::body::to_bytes(http_response.into_body()).await.expect("unable to extract body to bytes");
            """,
            *codegenScope,
        )
        if (body == "") {
            rustWriter.rustTemplate(
                """
                // No body.
                #{AssertEq}(&body, &bytes::Bytes::new());
                """,
                *codegenScope,
            )
        } else {
            assertOk(rustWriter) {
                rust(
                    "#T(&body, ${
                        rustWriter.escape(body).dq()
                    }, #T::from(${(mediaType ?: "unknown").dq()}))",
                    RuntimeType.protocolTest(codegenContext.runtimeConfig, "validate_body"),
                    RuntimeType.protocolTest(codegenContext.runtimeConfig, "MediaType"),
                )
            }
        }
    }

    private fun checkStatusCode(
        rustWriter: RustWriter,
        statusCode: Int,
    ) {
        rustWriter.rustTemplate(
            """
            #{AssertEq}(
                http::StatusCode::from_u16($statusCode).expect("invalid expected HTTP status code"),
                http_response.status()
            );
            """,
            *codegenScope,
        )
    }
}
