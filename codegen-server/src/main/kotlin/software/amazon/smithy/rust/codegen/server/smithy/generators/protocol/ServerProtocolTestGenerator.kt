/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators.protocol

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.protocoltests.traits.AppliesTo
import software.amazon.smithy.protocoltests.traits.HttpMalformedRequestTestCase
import software.amazon.smithy.protocoltests.traits.HttpMalformedRequestTestsTrait
import software.amazon.smithy.protocoltests.traits.HttpMalformedResponseBodyDefinition
import software.amazon.smithy.protocoltests.traits.HttpMalformedResponseDefinition
import software.amazon.smithy.protocoltests.traits.HttpRequestTestCase
import software.amazon.smithy.protocoltests.traits.HttpRequestTestsTrait
import software.amazon.smithy.protocoltests.traits.HttpResponseTestCase
import software.amazon.smithy.protocoltests.traits.HttpResponseTestsTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.core.smithy.transformers.allErrors
import software.amazon.smithy.rust.codegen.core.testutil.TokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType
import software.amazon.smithy.rust.codegen.server.smithy.generators.serverInstantiator
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerHttpBoundProtocolGenerator
import java.util.logging.Logger
import kotlin.reflect.KFunction1

private const val PROTOCOL_TEST_HELPER_MODULE_NAME = "protocol_test_helper"

/**
 * Generate protocol tests for an operation
 */
class ServerProtocolTestGenerator(
    private val codegenContext: CodegenContext,
    private val protocolSupport: ProtocolSupport,
    private val protocolGenerator: ServerProtocolGenerator,
) {
    private val logger = Logger.getLogger(javaClass.name)

    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val operationIndex = OperationIndex.of(codegenContext.model)

    private val serviceName = codegenContext.serviceShape.id.name.toPascalCase()
    private val operations =
        TopDownIndex.of(codegenContext.model).getContainedOperations(codegenContext.serviceShape).sortedBy { it.id }

    private val operationInputOutputTypes = operations.associateWith {
        val inputSymbol = symbolProvider.toSymbol(it.inputShape(model))
        val outputSymbol = symbolProvider.toSymbol(it.outputShape(model))
        val operationSymbol = symbolProvider.toSymbol(it)

        val inputT = inputSymbol.fullName
        val t = outputSymbol.fullName
        val outputT = if (it.errors.isEmpty()) {
            t
        } else {
            val errorType = RuntimeType("crate::error::${operationSymbol.name}Error")
            val e = errorType.fullyQualifiedName()
            "Result<$t, $e>"
        }

        inputT to outputT
    }

    private val instantiator = serverInstantiator(codegenContext)

    private val codegenScope = arrayOf(
        "Bytes" to RuntimeType.Bytes,
        "SmithyHttp" to RuntimeType.smithyHttp(codegenContext.runtimeConfig),
        "Http" to RuntimeType.Http,
        "Hyper" to RuntimeType.Hyper,
        "Tokio" to ServerCargoDependency.TokioDev.toType(),
        "Tower" to RuntimeType.Tower,
        "SmithyHttpServer" to ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
        "AssertEq" to RuntimeType.PrettyAssertions.resolve("assert_eq!"),
        "Router" to ServerRuntimeType.router(codegenContext.runtimeConfig),
    )

    sealed class TestCase {
        abstract val id: String
        abstract val documentation: String?
        abstract val protocol: ShapeId
        abstract val testType: TestType

        data class RequestTest(val testCase: HttpRequestTestCase, val operationShape: OperationShape) : TestCase() {
            override val id: String = testCase.id
            override val documentation: String? = testCase.documentation.orNull()
            override val protocol: ShapeId = testCase.protocol
            override val testType: TestType = TestType.Request
        }

        data class ResponseTest(val testCase: HttpResponseTestCase, val targetShape: StructureShape) : TestCase() {
            override val id: String = testCase.id
            override val documentation: String? = testCase.documentation.orNull()
            override val protocol: ShapeId = testCase.protocol
            override val testType: TestType = TestType.Response
        }

        data class MalformedRequestTest(val testCase: HttpMalformedRequestTestCase) : TestCase() {
            override val id: String = testCase.id
            override val documentation: String? = testCase.documentation.orNull()
            override val protocol: ShapeId = testCase.protocol
            override val testType: TestType = TestType.MalformedRequest
        }
    }

    fun render(writer: RustWriter) {
        renderTestHelper(writer)

        for (operation in operations) {
            protocolGenerator.renderOperation(writer, operation)
            renderOperationTestCases(operation, writer)
        }
    }

    /**
     * Render a test helper module to:
     *
     * - generate a dynamic builder for each handler, and
     * - construct a Tower service to exercise each test case.
     */
    private fun renderTestHelper(writer: RustWriter) {
        val operationNames = operations.map { it.toName() }
        val operationRegistryName = "OperationRegistry"
        val operationRegistryBuilderName = "${operationRegistryName}Builder"

        fun renderRegistryBuilderTypeParams() = writable {
            operations.forEach {
                val (inputT, outputT) = operationInputOutputTypes[it]!!
                writeInline("Fun<$inputT, $outputT>, (), ")
            }
        }

        fun renderRegistryBuilderMethods() = writable {
            operations.withIndex().forEach {
                val (inputT, outputT) = operationInputOutputTypes[it.value]!!
                val operationName = operationNames[it.index]
                rust(".$operationName((|_| Box::pin(async { todo!() })) as Fun<$inputT, $outputT> )")
            }
        }

        val module = RustModule.LeafModule(
            PROTOCOL_TEST_HELPER_MODULE_NAME,
            RustMetadata(
                additionalAttributes = listOf(
                    Attribute.Cfg("test"),
                    Attribute.AllowDeadCode,
                ),
                visibility = Visibility.PUBCRATE,
            ),
            inline = true,
        )

        writer.withInlineModule(module) {
            rustTemplate(
                """
                use #{Tower}::Service as _;

                pub(crate) type Fun<Input, Output> = fn(Input) -> std::pin::Pin<Box<dyn std::future::Future<Output = Output> + Send>>;

                type RegistryBuilder = crate::operation_registry::$operationRegistryBuilderName<#{Hyper}::Body, #{RegistryBuilderTypeParams:W}>;

                fn create_operation_registry_builder() -> RegistryBuilder {
                    crate::operation_registry::$operationRegistryBuilderName::default()
                        #{RegistryBuilderMethods:W}
                }

                pub(crate) async fn build_router_and_make_request(
                    http_request: #{Http}::request::Request<#{SmithyHttpServer}::body::Body>,
                    f: &dyn Fn(RegistryBuilder) -> RegistryBuilder,
                ) -> #{Http}::response::Response<#{SmithyHttpServer}::body::BoxBody> {
                    let mut router: #{Router} = f(create_operation_registry_builder())
                        .build()
                        .expect("unable to build operation registry")
                        .into();
                    let http_response = router
                        .call(http_request)
                        .await
                        .expect("unable to make an HTTP request");

                    http_response
                }

                /// The operation full name is a concatenation of `<operation namespace>.<operation name>`.
                pub(crate) fn check_operation_extension_was_set(http_response: #{Http}::response::Response<#{SmithyHttpServer}::body::BoxBody>, operation_full_name: &str) {
                    let operation_extension = http_response.extensions()
                        .get::<#{SmithyHttpServer}::extension::OperationExtension>()
                        .expect("extension `OperationExtension` not found");
                    #{AssertEq}(operation_extension.absolute(), operation_full_name);
                }
                """,
                "RegistryBuilderTypeParams" to renderRegistryBuilderTypeParams(),
                "RegistryBuilderMethods" to renderRegistryBuilderMethods(),
                *codegenScope,
            )
        }
    }

    private fun renderOperationTestCases(operationShape: OperationShape, writer: RustWriter) {
        val outputShape = operationShape.outputShape(codegenContext.model)
        val operationSymbol = symbolProvider.toSymbol(operationShape)

        val requestTests = operationShape.getTrait<HttpRequestTestsTrait>()
            ?.getTestCasesFor(AppliesTo.SERVER).orEmpty().map { TestCase.RequestTest(it, operationShape) }
        val responseTests = operationShape.getTrait<HttpResponseTestsTrait>()
            ?.getTestCasesFor(AppliesTo.SERVER).orEmpty().map { TestCase.ResponseTest(it, outputShape) }
        val errorTests = operationIndex.getErrors(operationShape).flatMap { error ->
            val testCases = error.getTrait<HttpResponseTestsTrait>()
                ?.getTestCasesFor(AppliesTo.SERVER).orEmpty()
            testCases.map { TestCase.ResponseTest(it, error) }
        }
        val malformedRequestTests = operationShape.getTrait<HttpMalformedRequestTestsTrait>()
            ?.testCases.orEmpty().map { TestCase.MalformedRequestTest(it) }
        val allTests: List<TestCase> = (requestTests + responseTests + errorTests + malformedRequestTests)
            .filterMatching()
            .fixBroken()

        if (allTests.isNotEmpty()) {
            val operationName = operationSymbol.name
            val module = RustModule.LeafModule(
                "server_${operationName.toSnakeCase()}_test",
                RustMetadata(
                    additionalAttributes = listOf(
                        Attribute.Cfg("test"),
                        Attribute.Custom("allow(unreachable_code, unused_variables)"),
                    ),
                    visibility = Visibility.PRIVATE,
                ),
                inline = true,
            )
            writer.withInlineModule(module) {
                renderAllTestCases(operationShape, allTests)
            }
        }
    }

    private fun RustWriter.renderAllTestCases(operationShape: OperationShape, allTests: List<TestCase>) {
        allTests.forEach {
            val operationSymbol = symbolProvider.toSymbol(operationShape)
            renderTestCaseBlock(it, this) {
                when (it) {
                    is TestCase.RequestTest -> this.renderHttpRequestTestCase(
                        it.testCase,
                        operationShape,
                        operationSymbol,
                    )

                    is TestCase.ResponseTest -> this.renderHttpResponseTestCase(
                        it.testCase,
                        it.targetShape,
                        operationShape,
                        operationSymbol,
                    )

                    is TestCase.MalformedRequestTest -> this.renderHttpMalformedRequestTestCase(
                        it.testCase,
                        operationShape,
                        operationSymbol,
                    )
                }
            }
        }
    }

    private fun OperationShape.toName(): String =
        RustReservedWords.escapeIfNeeded(symbolProvider.toSymbol(this).name.toSnakeCase())

    /**
     * Filter out test cases that are disabled or don't match the service protocol
     */
    private fun List<TestCase>.filterMatching(): List<TestCase> {
        return if (RunOnly.isNullOrEmpty()) {
            this.filter { testCase ->
                testCase.protocol == codegenContext.protocol &&
                    !DisableTests.contains(testCase.id)
            }
        } else {
            this.filter { RunOnly.contains(it.id) }
        }
    }

    // This function applies a "fix function" to each broken test before we synthesize it.
    // Broken tests are those whose definitions in the `awslabs/smithy` repository are wrong, usually because they have
    // not been written with a server-side perspective in mind.
    private fun List<TestCase>.fixBroken(): List<TestCase> = this.map {
        when (it) {
            is TestCase.RequestTest -> {
                val howToFixIt = BrokenRequestTests[Pair(codegenContext.serviceShape.id.toString(), it.id)]
                if (howToFixIt == null) {
                    it
                } else {
                    val fixed = howToFixIt(it.testCase, it.operationShape)
                    TestCase.RequestTest(fixed, it.operationShape)
                }
            }

            is TestCase.ResponseTest -> {
                val howToFixIt = BrokenResponseTests[Pair(codegenContext.serviceShape.id.toString(), it.id)]
                if (howToFixIt == null) {
                    it
                } else {
                    val fixed = howToFixIt(it.testCase)
                    TestCase.ResponseTest(fixed, it.targetShape)
                }
            }

            is TestCase.MalformedRequestTest -> {
                val howToFixIt = BrokenMalformedRequestTests[Pair(codegenContext.serviceShape.id.toString(), it.id)]
                if (howToFixIt == null) {
                    it
                } else {
                    val fixed = howToFixIt(it.testCase)
                    TestCase.MalformedRequestTest(fixed)
                }
            }
        }
    }

    private fun renderTestCaseBlock(
        testCase: TestCase,
        testModuleWriter: RustWriter,
        block: Writable,
    ) {
        testModuleWriter.newlinePrefix = "/// "
        if (testCase.documentation != null) {
            testModuleWriter.writeWithNoFormatting(testCase.documentation)
        }

        testModuleWriter.rust("Test ID: ${testCase.id}")
        testModuleWriter.newlinePrefix = ""

        TokioTest.render(testModuleWriter)

        if (expectFail(testCase)) {
            testModuleWriter.writeWithNoFormatting("#[should_panic]")
        }
        val fnNameSuffix = when (testCase.testType) {
            is TestType.Response -> "_response"
            is TestType.Request -> "_request"
            is TestType.MalformedRequest -> "_malformed_request"
        }
        testModuleWriter.rustBlock("async fn ${testCase.id.toSnakeCase()}$fnNameSuffix()") {
            block(this)
        }
    }

    /**
     * Renders an HTTP request test case.
     * We are given an HTTP request in the test case, and we assert that when we deserialize said HTTP request into
     * an operation's input shape, the resulting shape is of the form we expect, as defined in the test case.
     */
    private fun RustWriter.renderHttpRequestTestCase(
        httpRequestTestCase: HttpRequestTestCase,
        operationShape: OperationShape,
        operationSymbol: Symbol,
    ) {
        if (!protocolSupport.requestDeserialization) {
            rust("/* test case disabled for this protocol (not yet supported) */")
            return
        }

        // Test against original `OperationRegistryBuilder`.
        with(httpRequestTestCase) {
            renderHttpRequest(uri, method, headers, body.orNull(), queryParams, host.orNull())
        }
        if (protocolSupport.requestBodyDeserialization) {
            makeRequest(operationShape, this, checkRequestHandler(operationShape, httpRequestTestCase))
            checkHandlerWasEntered(operationShape, operationSymbol, this)
        }

        // Test against new service builder.
        with(httpRequestTestCase) {
            renderHttpRequest(uri, method, headers, body.orNull(), queryParams, host.orNull())
        }
        if (protocolSupport.requestBodyDeserialization) {
            makeRequest2(operationShape, operationSymbol, this, checkRequestHandler(operationShape, httpRequestTestCase))
            checkHandlerWasEntered2(this)
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

    private fun expectFail(testCase: TestCase): Boolean = ExpectFail.find {
        it.id == testCase.id && it.testType == testCase.testType && it.service == codegenContext.serviceShape.id.toString()
    } != null

    /**
     * Renders an HTTP response test case.
     * We are given an operation output shape or an error shape in the `params` field, and we assert that when we
     * serialize said shape, the resulting HTTP response is of the form we expect, as defined in the test case.
     * [shape] is either an operation output shape or an error shape.
     */
    private fun RustWriter.renderHttpResponseTestCase(
        testCase: HttpResponseTestCase,
        shape: StructureShape,
        operationShape: OperationShape,
        operationSymbol: Symbol,
    ) {
        val operationImplementationName =
            "${operationSymbol.name}${ServerHttpBoundProtocolGenerator.OPERATION_OUTPUT_WRAPPER_SUFFIX}"
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
        val operationImpl = if (operationShape.allErrors(model).isNotEmpty()) {
            if (shape.hasTrait<ErrorTrait>()) {
                val variant = symbolProvider.toSymbol(shape).name
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
    private fun RustWriter.renderHttpMalformedRequestTestCase(
        testCase: HttpMalformedRequestTestCase,
        operationShape: OperationShape,
        operationSymbol: Symbol,
    ) {
        val (_, outputT) = operationInputOutputTypes[operationShape]!!

        rust("// Use the `OperationRegistryBuilder`")
        rustBlock("") {
            with(testCase.request) {
                // TODO(https://github.com/awslabs/smithy/issues/1102): `uri` should probably not be an `Optional`.
                renderHttpRequest(uri.get(), method, headers, body.orNull(), queryParams, host.orNull())
            }
            makeRequest(operationShape, this, writable("todo!() as $outputT"))
            checkResponse(this, testCase.response)
        }

        rust("// Use new service builder")
        rustBlock("") {
            with(testCase.request) {
                // TODO(https://github.com/awslabs/smithy/issues/1102): `uri` should probably not be an `Optional`.
                renderHttpRequest(uri.get(), method, headers, body.orNull(), queryParams, host.orNull())
            }
            makeRequest2(operationShape, operationSymbol, this, writable("todo!() as $outputT"))
            checkResponse(this, testCase.response)
        }
    }

    private fun RustWriter.renderHttpRequest(
        uri: String,
        method: String,
        headers: Map<String, String>,
        body: String?,
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

                "#{SmithyHttpServer}::body::Body::from(#{Bytes}::from_static($sanitizedBody.as_bytes()))"
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

    /** Returns the body of the request test. */
    private fun checkRequestHandler(operationShape: OperationShape, httpRequestTestCase: HttpRequestTestCase) =
        writable {
            val inputShape = operationShape.inputShape(codegenContext.model)
            val outputShape = operationShape.outputShape(codegenContext.model)

            // Construct expected request.
            withBlock("let expected = ", ";") {
                instantiator.render(this, inputShape, httpRequestTestCase.params)
            }

            checkRequestParams(inputShape, this)

            // Construct a dummy response.
            withBlock("let response = ", ";") {
                instantiator.render(this, outputShape, Node.objectNode())
            }

            if (operationShape.errors.isEmpty()) {
                rust("response")
            } else {
                rust("Ok(response)")
            }
        }

    /** Checks the request using the `OperationRegistryBuilder`. */
    private fun makeRequest(
        operationShape: OperationShape,
        rustWriter: RustWriter,
        operationBody: Writable,
    ) {
        val (inputT, outputT) = operationInputOutputTypes[operationShape]!!

        rustWriter.withBlockTemplate(
            """
            let http_response = super::$PROTOCOL_TEST_HELPER_MODULE_NAME::build_router_and_make_request(
                http_request,
                &|builder| {
                    builder.${operationShape.toName()}((|input| Box::pin(async move {
            """,

            "})) as super::$PROTOCOL_TEST_HELPER_MODULE_NAME::Fun<$inputT, $outputT>)}).await;",
            *codegenScope,
        ) {
            operationBody()
        }
    }

    private fun checkHandlerWasEntered(
        operationShape: OperationShape,
        operationSymbol: Symbol,
        rustWriter: RustWriter,
    ) {
        val operationFullName = "${operationShape.id.namespace}.${operationSymbol.name}"
        rustWriter.rust(
            """
            super::$PROTOCOL_TEST_HELPER_MODULE_NAME::check_operation_extension_was_set(http_response, "$operationFullName");
            """,
        )
    }

    /** Checks the request using the new service builder. */
    private fun makeRequest2(
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
            let service = crate::service::$serviceName::builder_without_plugins::<#{Hyper}::body::Body>()
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

    private fun checkHandlerWasEntered2(rustWriter: RustWriter) {
        rustWriter.rust(
            """
            assert!(receiver.recv().await.is_some());
            """,
        )
    }

    private fun checkRequestParams(inputShape: StructureShape, rustWriter: RustWriter) {
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
            val hasFloatingPointMembers = inputShape.members().any {
                val target = model.expectShape(it.target)
                (target is DoubleShape) || (target is FloatShape)
            }

            // TODO(https://github.com/awslabs/smithy-rs/issues/1147) Handle the case of nested floating point members.
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

    private fun checkResponse(rustWriter: RustWriter, testCase: HttpResponseTestCase) {
        checkStatusCode(rustWriter, testCase.code)
        checkHeaders(rustWriter, "&http_response.headers()", testCase.headers)
        checkForbidHeaders(rustWriter, "&http_response.headers()", testCase.forbidHeaders)
        checkRequiredHeaders(rustWriter, "&http_response.headers()", testCase.requireHeaders)

        // We can't check that the `OperationExtension` is set in the response, because it is set in the implementation
        // of the operation `Handler` trait, a code path that does not get exercised when we don't have a request to
        // invoke it with (like in the case of an `httpResponseTest` test case).
        // In https://github.com/awslabs/smithy-rs/pull/1708: We did change `httpResponseTest`s generation to `call()`
        // the operation handler trait implementation instead of directly calling `from_request()`.

        // If no request body is defined, then no assertions are made about the body of the message.
        if (testCase.body.isPresent) {
            checkBody(rustWriter, testCase.body.get(), testCase.bodyMediaType.orNull())
        }
    }

    private fun checkResponse(rustWriter: RustWriter, testCase: HttpMalformedResponseDefinition) {
        checkStatusCode(rustWriter, testCase.code)
        checkHeaders(rustWriter, "&http_response.headers()", testCase.headers)

        // We can't check that the `OperationExtension` is set in the response, because it is set in the implementation
        // of the operation `Handler` trait, a code path that does not get exercised when we don't have a request to
        // invoke it with (like in the case of an `httpResponseTest` test case).
        // In https://github.com/awslabs/smithy-rs/pull/1708: We did change `httpResponseTest`s generation to `call()`
        // the operation handler trait implementation instead of directly calling `from_request()`.

        // If no request body is defined, then no assertions are made about the body of the message.
        if (testCase.body.isEmpty) return

        val httpMalformedResponseBodyDefinition = testCase.body.get()
        // From https://awslabs.github.io/smithy/1.0/spec/http-protocol-compliance-tests.html?highlight=httpresponsetest#httpmalformedresponsebodyassertion
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

    private fun checkBody(rustWriter: RustWriter, body: String, mediaType: String?) {
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
                #{AssertEq}(std::str::from_utf8(&body).unwrap(), "");
                """,
                *codegenScope,
            )
        } else {
            assertOk(rustWriter) {
                rustWriter.rust(
                    "#T(&body, ${
                    rustWriter.escape(body).dq()
                    }, #T::from(${(mediaType ?: "unknown").dq()}))",
                    RuntimeType.protocolTest(codegenContext.runtimeConfig, "validate_body"),
                    RuntimeType.protocolTest(codegenContext.runtimeConfig, "MediaType"),
                )
            }
        }
    }

    private fun checkStatusCode(rustWriter: RustWriter, statusCode: Int) {
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

    private fun checkRequiredHeaders(rustWriter: RustWriter, actualExpression: String, requireHeaders: List<String>) {
        basicCheck(
            requireHeaders,
            rustWriter,
            "required_headers",
            actualExpression,
            "require_headers",
        )
    }

    private fun checkForbidHeaders(rustWriter: RustWriter, actualExpression: String, forbidHeaders: List<String>) {
        basicCheck(
            forbidHeaders,
            rustWriter,
            "forbidden_headers",
            actualExpression,
            "forbid_headers",
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
                },
            )
        }
        assertOk(rustWriter) {
            rust(
                "#T($actualExpression, $variableName)",
                RuntimeType.protocolTest(codegenContext.runtimeConfig, "validate_headers"),
            )
        }
    }

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
            rustWriter.rust(
                "#T($actualExpression, $expectedVariableName)",
                RuntimeType.protocolTest(codegenContext.runtimeConfig, checkFunction),
            )
        }
    }

    /**
     * wraps `inner` in a call to `aws_smithy_protocol_test::assert_ok`, a convenience wrapper
     * for pretty printing protocol test helper results
     */
    private fun assertOk(rustWriter: RustWriter, inner: Writable) {
        rustWriter.rust("#T(", RuntimeType.protocolTest(codegenContext.runtimeConfig, "assert_ok"))
        inner(rustWriter)
        rustWriter.write(");")
    }

    private fun strSlice(writer: RustWriter, args: List<String>) {
        writer.withBlock("&[", "]") {
            rust(args.joinToString(",") { it.dq() })
        }
    }

    companion object {
        sealed class TestType {
            object Request : TestType()
            object Response : TestType()
            object MalformedRequest : TestType()
        }

        data class FailingTest(val service: String, val id: String, val testType: TestType)

        // These tests fail due to shortcomings in our implementation.
        // These could be configured via runtime configuration, but since this won't be long-lasting,
        // it makes sense to do the simplest thing for now.
        // The test will _fail_ if these pass, so we will discover & remove if we fix them by accident
        private const val AwsJson11 = "aws.protocoltests.json#JsonProtocol"
        private const val RestJson = "aws.protocoltests.restjson#RestJson"
        private const val RestJsonValidation = "aws.protocoltests.restjson.validation#RestJsonValidation"
        private const val MalformedRangeValidation = "aws.protocoltests.extras.restjson.validation#MalformedRangeValidation"
        private val ExpectFail: Set<FailingTest> = setOf(
            // Pending merge from the Smithy team: see https://github.com/awslabs/smithy/pull/1477.
            FailingTest(RestJson, "RestJsonWithPayloadExpectsImpliedContentType", TestType.MalformedRequest),

            // Pending resolution from the Smithy team, see https://github.com/awslabs/smithy/issues/1068.
            FailingTest(RestJson, "RestJsonHttpWithHeadersButNoPayload", TestType.Request),

            FailingTest(RestJson, "RestJsonHttpWithEmptyBlobPayload", TestType.Request),
            FailingTest(RestJson, "RestJsonHttpWithEmptyStructurePayload", TestType.Request),

            // See https://github.com/awslabs/smithy/issues/1098 for context.
            FailingTest(RestJson, "RestJsonHttpResponseCodeDefaultsToModeledCode", TestType.Response),

            // Endpoint trait is not implemented yet, see https://github.com/awslabs/smithy-rs/issues/950.
            FailingTest(RestJson, "RestJsonEndpointTrait", TestType.Request),
            FailingTest(RestJson, "RestJsonEndpointTraitWithHostLabel", TestType.Request),

            FailingTest(RestJson, "RestJsonWithBodyExpectsApplicationJsonContentType", TestType.MalformedRequest),
            FailingTest(RestJson, "RestJsonBodyMalformedListNullItem", TestType.MalformedRequest),
            FailingTest(RestJson, "RestJsonBodyMalformedMapNullValue", TestType.MalformedRequest),

            // Deprioritized, sets don't exist in Smithy 2.0.
            // They have the exact same semantics as list shapes with `@uniqueItems`,
            // so we could implement them as such once we've added support for constraint traits.
            //
            // See https://github.com/awslabs/smithy/issues/1266#issuecomment-1169543051.
            // See https://awslabs.github.io/smithy/2.0/guides/migrating-idl-1-to-2.html#convert-set-shapes-to-list-shapes.
            FailingTest(RestJson, "RestJsonMalformedSetDuplicateItems", TestType.MalformedRequest),
            FailingTest(RestJson, "RestJsonMalformedSetNullItem", TestType.MalformedRequest),
            FailingTest(RestJson, "RestJsonMalformedSetDuplicateBlobs", TestType.MalformedRequest),

            FailingTest(RestJson, "RestJsonMalformedUnionNoFieldsSet", TestType.MalformedRequest),

            // Tests involving constraint traits, which are not yet fully implemented.
            // See https://github.com/awslabs/smithy-rs/issues/1401.
            FailingTest(RestJsonValidation, "RestJsonMalformedLengthBlob_case0", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedLengthBlob_case1", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedRangeFloat_case0", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedRangeFloat_case1", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedRangeMaxFloat", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedRangeMinFloat", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedPatternSensitiveString", TestType.MalformedRequest),

            // See https://github.com/awslabs/smithy-rs/issues/1969
            FailingTest(MalformedRangeValidation, "RestJsonMalformedRangeShortOverride_case0", TestType.MalformedRequest),
            FailingTest(MalformedRangeValidation, "RestJsonMalformedRangeShortOverride_case1", TestType.MalformedRequest),
            FailingTest(
                MalformedRangeValidation,
                "RestJsonMalformedRangeIntegerOverride_case0",
                TestType.MalformedRequest,
            ),
            FailingTest(
                MalformedRangeValidation,
                "RestJsonMalformedRangeIntegerOverride_case1",
                TestType.MalformedRequest,
            ),
            FailingTest(
                MalformedRangeValidation,
                "RestJsonMalformedRangeLongOverride_case0",
                TestType.MalformedRequest,
            ),
            FailingTest(
                MalformedRangeValidation,
                "RestJsonMalformedRangeLongOverride_case1",
                TestType.MalformedRequest,
            ),
            FailingTest(MalformedRangeValidation, "RestJsonMalformedRangeMaxShortOverride", TestType.MalformedRequest),
            FailingTest(
                MalformedRangeValidation,
                "RestJsonMalformedRangeMaxIntegerOverride",
                TestType.MalformedRequest,
            ),
            FailingTest(MalformedRangeValidation, "RestJsonMalformedRangeMaxLongOverride", TestType.MalformedRequest),
            FailingTest(MalformedRangeValidation, "RestJsonMalformedRangeMinShortOverride", TestType.MalformedRequest),
            FailingTest(
                MalformedRangeValidation,
                "RestJsonMalformedRangeMinIntegerOverride",
                TestType.MalformedRequest,
            ),
            FailingTest(MalformedRangeValidation, "RestJsonMalformedRangeMinLongOverride", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedRangeByteOverride_case0", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedRangeByteOverride_case1", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedRangeFloatOverride_case0", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedRangeFloatOverride_case1", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedLengthMaxStringOverride", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedLengthMinStringOverride", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedRangeMaxByteOverride", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedRangeMaxFloatOverride", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedRangeMinByteOverride", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedRangeMinFloatOverride", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedPatternListOverride_case0", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedPatternListOverride_case1", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedPatternMapKeyOverride_case0", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedPatternMapKeyOverride_case1", TestType.MalformedRequest),
            FailingTest(
                RestJsonValidation,
                "RestJsonMalformedPatternMapValueOverride_case0",
                TestType.MalformedRequest,
            ),
            FailingTest(
                RestJsonValidation,
                "RestJsonMalformedPatternMapValueOverride_case1",
                TestType.MalformedRequest,
            ),
            FailingTest(RestJsonValidation, "RestJsonMalformedPatternStringOverride_case0", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedPatternStringOverride_case1", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedPatternUnionOverride_case0", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedPatternUnionOverride_case1", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedLengthBlobOverride_case0", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedLengthBlobOverride_case1", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedLengthListOverride_case0", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedLengthListOverride_case1", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedLengthMapOverride_case0", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedLengthMapOverride_case1", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedLengthStringOverride_case0", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedLengthStringOverride_case1", TestType.MalformedRequest),
            FailingTest(RestJsonValidation, "RestJsonMalformedLengthStringOverride_case2", TestType.MalformedRequest),

            // Some tests for the S3 service (restXml).
            FailingTest("com.amazonaws.s3#AmazonS3", "GetBucketLocationUnwrappedOutput", TestType.Response),
            FailingTest("com.amazonaws.s3#AmazonS3", "S3DefaultAddressing", TestType.Request),
            FailingTest("com.amazonaws.s3#AmazonS3", "S3VirtualHostAddressing", TestType.Request),
            FailingTest("com.amazonaws.s3#AmazonS3", "S3PathAddressing", TestType.Request),
            FailingTest("com.amazonaws.s3#AmazonS3", "S3VirtualHostDualstackAddressing", TestType.Request),
            FailingTest("com.amazonaws.s3#AmazonS3", "S3VirtualHostAccelerateAddressing", TestType.Request),
            FailingTest("com.amazonaws.s3#AmazonS3", "S3VirtualHostDualstackAccelerateAddressing", TestType.Request),
            FailingTest("com.amazonaws.s3#AmazonS3", "S3OperationAddressingPreferred", TestType.Request),

            // AwsJson1.0 failing tests.
            FailingTest("aws.protocoltests.json10#JsonRpc10", "AwsJson10EndpointTraitWithHostLabel", TestType.Request),
            FailingTest("aws.protocoltests.json10#JsonRpc10", "AwsJson10EndpointTrait", TestType.Request),

            // AwsJson1.1 failing tests.
            FailingTest("aws.protocoltests.json#JsonProtocol", "AwsJson11EndpointTraitWithHostLabel", TestType.Request),
            FailingTest("aws.protocoltests.json#JsonProtocol", "AwsJson11EndpointTrait", TestType.Request),
            FailingTest("aws.protocoltests.json#JsonProtocol", "parses_httpdate_timestamps", TestType.Response),
            FailingTest("aws.protocoltests.json#JsonProtocol", "parses_iso8601_timestamps", TestType.Response),
            FailingTest(
                "aws.protocoltests.json#JsonProtocol",
                "parses_the_request_id_from_the_response",
                TestType.Response,
            ),

        )
        private val RunOnly: Set<String>? = null

        // These tests are not even attempted to be generated, either because they will not compile
        // or because they are flaky
        private val DisableTests = setOf<String>()

        private fun fixRestJsonSupportsNaNFloatQueryValues(
            testCase: HttpRequestTestCase,
            @Suppress("UNUSED_PARAMETER")
            operationShape: OperationShape,
        ): HttpRequestTestCase {
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
                """.trimIndent(),
            ).asObjectNode().get()

            return testCase.toBuilder().params(params).build()
        }

        private fun fixRestJsonSupportsInfinityFloatQueryValues(
            testCase: HttpRequestTestCase,
            @Suppress("UNUSED_PARAMETER")
            operationShape: OperationShape,
        ): HttpRequestTestCase =
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
                    """.trimMargin(),
                ).asObjectNode().get(),
            ).build()

        private fun fixRestJsonSupportsNegativeInfinityFloatQueryValues(
            testCase: HttpRequestTestCase,
            @Suppress("UNUSED_PARAMETER")
            operationShape: OperationShape,
        ): HttpRequestTestCase =
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
                    """.trimMargin(),
                ).asObjectNode().get(),
            ).build()

        private fun fixRestJsonAllQueryStringTypes(
            testCase: HttpRequestTestCase,
            @Suppress("UNUSED_PARAMETER")
            operationShape: OperationShape,
        ): HttpRequestTestCase =
            testCase.toBuilder().params(
                Node.parse(
                    """
                    {
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
                    }
                    """.trimMargin(),
                ).asObjectNode().get(),
            ).build()

        private fun fixRestJsonQueryStringEscaping(
            testCase: HttpRequestTestCase,
            @Suppress("UNUSED_PARAMETER")
            operationShape: OperationShape,
        ): HttpRequestTestCase =
            testCase.toBuilder().params(
                Node.parse(
                    """
                    {
                        "queryString": "%:/?#[]@!${'$'}&'()*+,;=😹",
                        "queryParamsMapOfStringList": {
                            "String": ["%:/?#[]@!${'$'}&'()*+,;=😹"]
                        }
                    }
                    """.trimMargin(),
                ).asObjectNode().get(),
            ).build()

        private fun fixAwsJson11MissingHeaderXAmzTarget(
            testCase: HttpRequestTestCase,
            operationShape: OperationShape,
        ): HttpRequestTestCase =
            testCase.toBuilder().putHeader("x-amz-target", "JsonProtocol.${operationShape.id.name}").build()

        private fun fixRestJsonInvalidGreetingError(testCase: HttpResponseTestCase): HttpResponseTestCase =
            testCase.toBuilder().putHeader("X-Amzn-Errortype", "aws.protocoltests.restjson#InvalidGreeting").build()
        private fun fixRestJsonEmptyComplexErrorWithNoMessage(testCase: HttpResponseTestCase): HttpResponseTestCase =
            testCase.toBuilder().putHeader("X-Amzn-Errortype", "aws.protocoltests.restjson#ComplexError").build()
        private fun fixRestJsonComplexErrorWithNoMessage(testCase: HttpResponseTestCase): HttpResponseTestCase =
            testCase.toBuilder().putHeader("X-Amzn-Errortype", "aws.protocoltests.restjson#ComplexError").build()

        // TODO(https://github.com/awslabs/smithy/issues/1506)
        private fun fixRestJsonMalformedPatternReDOSString(testCase: HttpMalformedRequestTestCase): HttpMalformedRequestTestCase {
            val brokenResponse = testCase.response
            val brokenBody = brokenResponse.body.get()
            val fixedBody = HttpMalformedResponseBodyDefinition.builder()
                .mediaType(brokenBody.mediaType)
                .contents(
                    """
                    {
                        "message" : "1 validation error detected. Value 000000000000000000000000000000000000000000000000000000000000000000000000000000000000! at '/evilString' failed to satisfy constraint: Member must satisfy regular expression pattern: ^([0-9]+)+${'$'}",
                        "fieldList" : [{"message": "Value 000000000000000000000000000000000000000000000000000000000000000000000000000000000000! at '/evilString' failed to satisfy constraint: Member must satisfy regular expression pattern: ^([0-9]+)+${'$'}", "path": "/evilString"}]
                    }
                    """.trimIndent(),
                )
                .build()

            return testCase.toBuilder()
                .response(brokenResponse.toBuilder().body(fixedBody).build())
                .build()
        }

        // These are tests whose definitions in the `awslabs/smithy` repository are wrong.
        // This is because they have not been written from a server perspective, and as such the expected `params` field is incomplete.
        // TODO(https://github.com/awslabs/smithy-rs/issues/1288): Contribute a PR to fix them upstream.
        private val BrokenRequestTests = mapOf(
            // https://github.com/awslabs/smithy/pull/1040
            Pair(RestJson, "RestJsonSupportsNaNFloatQueryValues") to ::fixRestJsonSupportsNaNFloatQueryValues,
            Pair(RestJson, "RestJsonSupportsInfinityFloatQueryValues") to ::fixRestJsonSupportsInfinityFloatQueryValues,
            Pair(
                RestJson,
                "RestJsonSupportsNegativeInfinityFloatQueryValues",
            ) to ::fixRestJsonSupportsNegativeInfinityFloatQueryValues,
            Pair(RestJson, "RestJsonAllQueryStringTypes") to ::fixRestJsonAllQueryStringTypes,
            Pair(RestJson, "RestJsonQueryStringEscaping") to ::fixRestJsonQueryStringEscaping,

            // https://github.com/awslabs/smithy/pull/1392
            // Missing `X-Amz-Target` in response header
            Pair(AwsJson11, "AwsJson11Enums") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "AwsJson11ListsSerializeNull") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "AwsJson11MapsSerializeNullValues") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(
                AwsJson11,
                "AwsJson11ServersDontDeserializeNullStructureValues",
            ) to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "PutAndGetInlineDocumentsInput") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(
                AwsJson11,
                "json_1_1_client_sends_empty_payload_for_no_input_shape",
            ) to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(
                AwsJson11,
                "json_1_1_service_supports_empty_payload_for_no_input_shape",
            ) to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "sends_requests_to_slash") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_blob_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_boolean_shapes_false") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_boolean_shapes_true") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_double_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_empty_list_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_empty_map_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_empty_structure_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_float_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_integer_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_list_of_map_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_list_of_recursive_structure_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_list_of_structure_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_list_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_long_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_map_of_list_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_map_of_recursive_structure_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_map_of_structure_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_map_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_recursive_structure_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_string_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_string_shapes_with_jsonvalue_trait") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(
                AwsJson11,
                "serializes_structure_members_with_locationname_traits",
            ) to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_structure_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_structure_which_have_no_members") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(AwsJson11, "serializes_timestamp_shapes") to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(
                AwsJson11,
                "serializes_timestamp_shapes_with_httpdate_timestampformat",
            ) to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(
                AwsJson11,
                "serializes_timestamp_shapes_with_iso8601_timestampformat",
            ) to ::fixAwsJson11MissingHeaderXAmzTarget,
            Pair(
                AwsJson11,
                "serializes_timestamp_shapes_with_unixtimestamp_timestampformat",
            ) to ::fixAwsJson11MissingHeaderXAmzTarget,
        )

        private val BrokenResponseTests: Map<Pair<String, String>, KFunction1<HttpResponseTestCase, HttpResponseTestCase>> =
            // TODO(https://github.com/awslabs/smithy/issues/1494)
            mapOf(
                Pair(RestJson, "RestJsonInvalidGreetingError") to ::fixRestJsonInvalidGreetingError,
                Pair(RestJson, "RestJsonEmptyComplexErrorWithNoMessage") to ::fixRestJsonEmptyComplexErrorWithNoMessage,
                Pair(RestJson, "RestJsonComplexErrorWithNoMessage") to ::fixRestJsonComplexErrorWithNoMessage,
            )

        private val BrokenMalformedRequestTests: Map<Pair<String, String>, KFunction1<HttpMalformedRequestTestCase, HttpMalformedRequestTestCase>> =
            // TODO(https://github.com/awslabs/smithy/issues/1506)
            mapOf(
                Pair(RestJsonValidation, "RestJsonMalformedPatternReDOSString") to ::fixRestJsonMalformedPatternReDOSString,
            )
    }
}
