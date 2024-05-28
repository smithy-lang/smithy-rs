/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.protocoltests.traits.AppliesTo
import software.amazon.smithy.protocoltests.traits.HttpMessageTestCase
import software.amazon.smithy.protocoltests.traits.HttpRequestTestCase
import software.amazon.smithy.protocoltests.traits.HttpRequestTestsTrait
import software.amazon.smithy.protocoltests.traits.HttpResponseTestCase
import software.amazon.smithy.protocoltests.traits.HttpResponseTestsTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.generators.ClientInstantiator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.allow
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import java.util.logging.Logger
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType as RT

data class ClientCreationParams(
    val codegenContext: ClientCodegenContext,
    val httpClientName: String,
    val configBuilderName: String,
    val clientName: String,
)

interface ProtocolTestGenerator {
    val codegenContext: ClientCodegenContext
    val protocolSupport: ProtocolSupport
    val operationShape: OperationShape

    fun render(writer: RustWriter)
}

/**
 * Generate protocol tests for an operation
 */
class DefaultProtocolTestGenerator(
    override val codegenContext: ClientCodegenContext,
    override val protocolSupport: ProtocolSupport,
    override val operationShape: OperationShape,
    private val renderClientCreation: RustWriter.(ClientCreationParams) -> Unit = { params ->
        rustTemplate(
            """
            let ${params.clientName} = #{Client}::from_conf(
                ${params.configBuilderName}
                    .http_client(${params.httpClientName})
                    .build()
            );
            """,
            "Client" to ClientRustModule.root.toType().resolve("Client"),
        )
    },
) : ProtocolTestGenerator {
    private val rc = codegenContext.runtimeConfig
    private val logger = Logger.getLogger(javaClass.name)

    private val inputShape = operationShape.inputShape(codegenContext.model)
    private val outputShape = operationShape.outputShape(codegenContext.model)
    private val operationSymbol = codegenContext.symbolProvider.toSymbol(operationShape)
    private val operationIndex = OperationIndex.of(codegenContext.model)

    private val instantiator = ClientInstantiator(codegenContext)

    private val codegenScope =
        arrayOf(
            "SmithyHttp" to RT.smithyHttp(rc),
            "AssertEq" to RT.PrettyAssertions.resolve("assert_eq!"),
            "Uri" to RT.Http.resolve("Uri"),
        )

    sealed class TestCase {
        abstract val testCase: HttpMessageTestCase

        data class RequestTest(override val testCase: HttpRequestTestCase) : TestCase()

        data class ResponseTest(override val testCase: HttpResponseTestCase, val targetShape: StructureShape) :
            TestCase()
    }

    override fun render(writer: RustWriter) {
        val requestTests =
            operationShape.getTrait<HttpRequestTestsTrait>()
                ?.getTestCasesFor(AppliesTo.CLIENT).orEmpty().map { TestCase.RequestTest(it) }
        val responseTests =
            operationShape.getTrait<HttpResponseTestsTrait>()
                ?.getTestCasesFor(AppliesTo.CLIENT).orEmpty().map { TestCase.ResponseTest(it, outputShape) }
        val errorTests =
            operationIndex.getErrors(operationShape).flatMap { error ->
                val testCases =
                    error.getTrait<HttpResponseTestsTrait>()
                        ?.getTestCasesFor(AppliesTo.CLIENT).orEmpty()
                testCases.map { TestCase.ResponseTest(it, error) }
            }
        val allTests: List<TestCase> = (requestTests + responseTests + errorTests).filterMatching()
        if (allTests.isNotEmpty()) {
            val operationName = operationSymbol.name
            val testModuleName = "${operationName.toSnakeCase()}_request_test"
            val additionalAttributes =
                listOf(
                    Attribute(allow("unreachable_code", "unused_variables")),
                )
            writer.withInlineModule(
                RustModule.inlineTests(testModuleName, additionalAttributes = additionalAttributes),
                null,
            ) {
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
                testCase.testCase.protocol == codegenContext.protocol &&
                    !DisableTests.contains(testCase.testCase.id)
            }
        } else {
            this.filter { RunOnly.contains(it.testCase.id) }
        }
    }

    private fun renderTestCaseBlock(
        testCase: HttpMessageTestCase,
        testModuleWriter: RustWriter,
        block: Writable,
    ) {
        testModuleWriter.newlinePrefix = "/// "
        testCase.documentation.map {
            testModuleWriter.writeWithNoFormatting(it)
        }
        testModuleWriter.write("Test ID: ${testCase.id}")
        testModuleWriter.newlinePrefix = ""
        Attribute.TokioTest.render(testModuleWriter)
        val action =
            when (testCase) {
                is HttpResponseTestCase -> Action.Response
                is HttpRequestTestCase -> Action.Request
                else -> throw CodegenException("unknown test case type")
            }
        if (expectFail(testCase)) {
            testModuleWriter.writeWithNoFormatting("#[should_panic]")
        }
        val fnName =
            when (action) {
                is Action.Response -> "_response"
                is Action.Request -> "_request"
            }
        Attribute.AllowUnusedMut.render(testModuleWriter)
        testModuleWriter.rustBlock("async fn ${testCase.id.toSnakeCase()}$fnName()") {
            block(this)
        }
    }

    private fun RustWriter.renderHttpRequestTestCase(httpRequestTestCase: HttpRequestTestCase) {
        if (!protocolSupport.requestSerialization) {
            rust("/* test case disabled for this protocol (not yet supported) */")
            return
        }
        val customParams =
            httpRequestTestCase.vendorParams.getObjectMember("endpointParams").orNull()?.let { params ->
                writable {
                    val customizations = codegenContext.rootDecorator.endpointCustomizations(codegenContext)
                    params.getObjectMember("builtInParams").orNull()?.members?.forEach { (name, value) ->
                        customizations.firstNotNullOf {
                            it.setBuiltInOnServiceConfig(name.value, value, "config_builder")
                        }(this)
                    }
                }
            } ?: writable { }
        // support test cases that set the host value, e.g: https://github.com/smithy-lang/smithy/blob/be68f3bbdfe5bf50a104b387094d40c8069f16b1/smithy-aws-protocol-tests/model/restJson1/endpoint-paths.smithy#L19
        val host = "https://${httpRequestTestCase.host.orNull() ?: "example.com"}".dq()
        rustTemplate(
            """
            let (http_client, request_receiver) = #{capture_request}(None);
            let config_builder = #{config}::Config::builder().with_test_defaults().endpoint_url($host);
            #{customParams}

            """,
            "capture_request" to
                CargoDependency.smithyRuntimeTestUtil(rc).toType()
                    .resolve("client::http::test_util::capture_request"),
            "config" to ClientRustModule.config,
            "customParams" to customParams,
        )
        renderClientCreation(this, ClientCreationParams(codegenContext, "http_client", "config_builder", "client"))

        writeInline("let result = ")
        instantiator.renderFluentCall(this, "client", operationShape, inputShape, httpRequestTestCase.params)
        rust(""".send().await;""")
        // Response parsing will always fail since we feed it an empty response body, so we don't care
        // if it fails, but it is helpful to print what that failure was for debugging
        rust("let _ = dbg!(result);")
        rust("""let http_request = request_receiver.expect_request();""")

        checkQueryParams(this, httpRequestTestCase.queryParams)
        checkForbidQueryParams(this, httpRequestTestCase.forbidQueryParams)
        checkRequiredQueryParams(this, httpRequestTestCase.requireQueryParams)
        checkHeaders(this, "http_request.headers()", httpRequestTestCase.headers)
        checkForbidHeaders(this, "http_request.headers()", httpRequestTestCase.forbidHeaders)
        checkRequiredHeaders(this, "http_request.headers()", httpRequestTestCase.requireHeaders)

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

            rustTemplate(
                """
                let uri: #{Uri} = http_request.uri().parse().expect("invalid URI sent");
                #{AssertEq}(http_request.method(), ${method.dq()}, "method was incorrect");
                #{AssertEq}(uri.path(), ${uri.dq()}, "path was incorrect");
                """,
                *codegenScope,
            )

            resolvedHost.orNull()?.also { host ->
                rustTemplate(
                    """#{AssertEq}(uri.host().expect("host should be set"), ${host.dq()});""",
                    *codegenScope,
                )
            }
        }
    }

    private fun HttpMessageTestCase.action(): Action =
        when (this) {
            is HttpRequestTestCase -> Action.Request
            is HttpResponseTestCase -> Action.Response
            else -> throw CodegenException("Unknown test case type")
        }

    private fun expectFail(testCase: HttpMessageTestCase): Boolean =
        ExpectFail.find {
            it.id == testCase.id && it.action == testCase.action() && it.service == codegenContext.serviceShape.id.toString()
        } != null

    private fun RustWriter.renderHttpResponseTestCase(
        testCase: HttpResponseTestCase,
        expectedShape: StructureShape,
    ) {
        if (!protocolSupport.responseDeserialization || (
                !protocolSupport.errorDeserialization &&
                    expectedShape.hasTrait(
                        ErrorTrait::class.java,
                    )
            )
        ) {
            rust("/* test case disabled for this protocol (not yet supported) */")
            return
        }
        writeInline("let expected_output =")
        instantiator.render(this, expectedShape, testCase.params)
        write(";")
        rustTemplate(
            "let mut http_response = #{Response}::try_from(#{HttpResponseBuilder}::new()",
            "Response" to RT.smithyRuntimeApi(rc).resolve("http::Response"),
            "HttpResponseBuilder" to RT.HttpResponseBuilder,
        )
        testCase.headers.forEach { (key, value) ->
            writeWithNoFormatting(".header(${key.dq()}, ${value.dq()})")
        }
        rust(
            """
            .status(${testCase.code})
            .body(#T::from(${testCase.body.orNull()?.dq()?.replace("#", "##") ?: "vec![]"}))
            .unwrap()
            ).unwrap();
            """,
            RT.sdkBody(runtimeConfig = rc),
        )
        rustTemplate(
            """
            use #{DeserializeResponse};
            use #{RuntimePlugin};

            let op = #{Operation}::new();
            let config = op.config().expect("the operation has config");
            let de = config.load::<#{SharedResponseDeserializer}>().expect("the config must have a deserializer");

            let parsed = de.deserialize_streaming(&mut http_response);
            let parsed = parsed.unwrap_or_else(|| {
                let http_response = http_response.map(|body| {
                    #{SdkBody}::from(#{copy_from_slice}(body.bytes().unwrap()))
                });
                de.deserialize_nonstreaming(&http_response)
            });
            """,
            "copy_from_slice" to RT.Bytes.resolve("copy_from_slice"),
            "SharedResponseDeserializer" to
                RT.smithyRuntimeApiClient(rc)
                    .resolve("client::ser_de::SharedResponseDeserializer"),
            "Operation" to codegenContext.symbolProvider.toSymbol(operationShape),
            "DeserializeResponse" to RT.smithyRuntimeApiClient(rc).resolve("client::ser_de::DeserializeResponse"),
            "RuntimePlugin" to RT.runtimePlugin(rc),
            "SdkBody" to RT.sdkBody(rc),
        )
        if (expectedShape.hasTrait<ErrorTrait>()) {
            val errorSymbol = codegenContext.symbolProvider.symbolForOperationError(operationShape)
            val errorVariant = codegenContext.symbolProvider.toSymbol(expectedShape).name
            rust("""let parsed = parsed.expect_err("should be error response");""")
            rustTemplate(
                """let parsed: &#{Error} = parsed.as_operation_error().expect("operation error").downcast_ref().unwrap();""",
                "Error" to codegenContext.symbolProvider.symbolForOperationError(operationShape),
            )
            rustBlock("if let #T::$errorVariant(parsed) = parsed", errorSymbol) {
                compareMembers(expectedShape)
            }
            rustBlock("else") {
                rust("panic!(\"wrong variant: Got: {:?}. Expected: {:?}\", parsed, expected_output);")
            }
        } else {
            rustTemplate(
                """let parsed = parsed.expect("should be successful response").downcast::<#{Output}>().unwrap();""",
                "Output" to codegenContext.symbolProvider.toSymbol(expectedShape),
            )
            compareMembers(outputShape)
        }
    }

    private fun RustWriter.compareMembers(shape: StructureShape) {
        shape.members().forEach { member ->
            val memberName = codegenContext.symbolProvider.toMemberName(member)
            if (member.isStreaming(codegenContext.model)) {
                rustTemplate(
                    """
                    #{AssertEq}(
                        parsed.$memberName.collect().await.unwrap().into_bytes(),
                        expected_output.$memberName.collect().await.unwrap().into_bytes()
                    );
                    """,
                    *codegenScope,
                )
            } else {
                when (codegenContext.model.expectShape(member.target)) {
                    is DoubleShape, is FloatShape -> {
                        addUseImports(RT.protocolTest(rc, "FloatEquals").toSymbol())
                        rust(
                            """
                            assert!(parsed.$memberName.float_equals(&expected_output.$memberName),
                                "Unexpected value for `$memberName` {:?} vs. {:?}", expected_output.$memberName, parsed.$memberName);
                            """,
                        )
                    }

                    else ->
                        rustTemplate(
                            """#{AssertEq}(parsed.$memberName, expected_output.$memberName, "Unexpected value for `$memberName`");""",
                            *codegenScope,
                        )
                }
            }
        }
    }

    private fun checkBody(
        rustWriter: RustWriter,
        body: String,
        mediaType: String?,
    ) {
        rustWriter.write("""let body = http_request.body().bytes().expect("body should be strict");""")
        if (body == "") {
            rustWriter.rustTemplate(
                """
                // No body
                #{AssertEq}(::std::str::from_utf8(body).unwrap(), "");
                """,
                *codegenScope,
            )
        } else {
            // When we generate a body instead of a stub, drop the trailing `;` and enable the assertion
            assertOk(rustWriter) {
                rustWriter.write(
                    "#T(body, ${
                        rustWriter.escape(body).dq()
                    }, #T::from(${(mediaType ?: "unknown").dq()}))",
                    RT.protocolTest(rc, "validate_body"),
                    RT.protocolTest(rc, "MediaType"),
                )
            }
        }
    }

    private fun checkRequiredHeaders(
        rustWriter: RustWriter,
        actualExpression: String,
        requireHeaders: List<String>,
    ) {
        basicCheck(
            requireHeaders,
            rustWriter,
            "required_headers",
            actualExpression,
            "require_headers",
        )
    }

    private fun checkForbidHeaders(
        rustWriter: RustWriter,
        actualExpression: String,
        forbidHeaders: List<String>,
    ) {
        basicCheck(
            forbidHeaders,
            rustWriter,
            "forbidden_headers",
            actualExpression,
            "forbid_headers",
        )
    }

    private fun checkHeaders(
        rustWriter: RustWriter,
        actualExpression: String,
        headers: Map<String, String>,
    ) {
        if (headers.isEmpty()) {
            return
        }
        val variableName = "expected_headers"
        rustWriter.withBlock("let $variableName = [", "];") {
            writeWithNoFormatting(
                headers.entries.joinToString(",") {
                    "(${it.key.dq()}, ${it.value.dq()})"
                },
            )
        }
        assertOk(rustWriter) {
            write(
                "#T($actualExpression, $variableName)",
                RT.protocolTest(rc, "validate_headers"),
            )
        }
    }

    private fun checkRequiredQueryParams(
        rustWriter: RustWriter,
        requiredParams: List<String>,
    ) = basicCheck(
        requiredParams,
        rustWriter,
        "required_params",
        "&http_request",
        "require_query_params",
    )

    private fun checkForbidQueryParams(
        rustWriter: RustWriter,
        forbidParams: List<String>,
    ) = basicCheck(
        forbidParams,
        rustWriter,
        "forbid_params",
        "&http_request",
        "forbid_query_params",
    )

    private fun checkQueryParams(
        rustWriter: RustWriter,
        queryParams: List<String>,
    ) = basicCheck(
        queryParams,
        rustWriter,
        "expected_query_params",
        "&http_request",
        "validate_query_string",
    )

    private fun basicCheck(
        params: List<String>,
        rustWriter: RustWriter,
        expectedVariableName: String,
        actualExpression: String,
        checkFunction: String,
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
                RT.protocolTest(rc, checkFunction),
            )
        }
    }

    /**
     * wraps `inner` in a call to `aws_smithy_protocol_test::assert_ok`, a convenience wrapper
     * for pretty printing protocol test helper results
     */
    private fun assertOk(
        rustWriter: RustWriter,
        inner: Writable,
    ) {
        rustWriter.write("#T(", RT.protocolTest(rc, "assert_ok"))
        inner(rustWriter)
        rustWriter.write(");")
    }

    private fun strSlice(
        writer: RustWriter,
        args: List<String>,
    ) {
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
        private val ExpectFail =
            setOf<FailingTest>(
                // Failing because we don't serialize default values if they match the default
                FailingTest(JsonRpc10, "AwsJson10ClientPopulatesDefaultsValuesWhenMissingInResponse", Action.Request),
                FailingTest(JsonRpc10, "AwsJson10ClientUsesExplicitlyProvidedMemberValuesOverDefaults", Action.Request),
                FailingTest(JsonRpc10, "AwsJson10ClientPopulatesDefaultValuesInInput", Action.Request),
            )
        private val RunOnly: Set<String>? = null

        // These tests are not even attempted to be generated, either because they will not compile
        // or because they are flaky
        private val DisableTests: Set<String> = setOf()
    }
}
