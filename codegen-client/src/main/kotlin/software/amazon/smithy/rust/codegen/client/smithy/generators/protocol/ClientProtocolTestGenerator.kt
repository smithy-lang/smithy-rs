/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import software.amazon.smithy.aws.traits.auth.SigV4Trait
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.protocoltests.traits.AppliesTo
import software.amazon.smithy.protocoltests.traits.HttpRequestTestCase
import software.amazon.smithy.protocoltests.traits.HttpResponseTestCase
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.auth.AuthSchemeResolverGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ClientInstantiator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.BrokenTest
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.FailingTest
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolTestGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ServiceShapeId.AWS_JSON_10
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ServiceShapeId.REST_JSON
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ServiceShapeId.REST_XML
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ServiceShapeId.RPC_V2_CBOR
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.TestCase
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isStreaming
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.outputShape
import java.util.logging.Logger
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType as RT

data class ClientCreationParams(
    val codegenContext: ClientCodegenContext,
    val httpClientName: String,
    val configBuilderName: String,
    val clientName: String,
)

/**
 * Generate client protocol tests for an [operationShape].
 */
class ClientProtocolTestGenerator(
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
) : ProtocolTestGenerator() {
    companion object {
        private val ExpectFail =
            setOf<FailingTest>(
                // Failing because we don't serialize default values if they match the default.
                FailingTest.ResponseTest(AWS_JSON_10, "AwsJson10ClientPopulatesDefaultsValuesWhenMissingInResponse"),
                FailingTest.ResponseTest(
                    AWS_JSON_10,
                    "AwsJson10ClientErrorCorrectsWithDefaultValuesWhenServerFailsToSerializeRequiredValues",
                ),
                FailingTest.RequestTest(AWS_JSON_10, "AwsJson10ClientUsesExplicitlyProvidedMemberValuesOverDefaults"),
                FailingTest.RequestTest(AWS_JSON_10, "AwsJson10ClientPopulatesDefaultValuesInInput"),
                FailingTest.RequestTest(REST_JSON, "RestJsonClientPopulatesDefaultValuesInInput"),
                FailingTest.RequestTest(REST_JSON, "RestJsonClientUsesExplicitlyProvidedMemberValuesOverDefaults"),
                FailingTest.ResponseTest(REST_JSON, "RestJsonClientPopulatesDefaultsValuesWhenMissingInResponse"),
                FailingTest.RequestTest(RPC_V2_CBOR, "RpcV2CborClientPopulatesDefaultValuesInInput"),
                FailingTest.ResponseTest(
                    RPC_V2_CBOR,
                    "RpcV2CborClientPopulatesDefaultsValuesWhenMissingInResponse",
                ),
                FailingTest.RequestTest(RPC_V2_CBOR, "RpcV2CborClientUsesExplicitlyProvidedMemberValuesOverDefaults"),
                // Failing due to bug in httpPreficHeaders serialization
                // https://github.com/smithy-lang/smithy-rs/issues/4184
                FailingTest.RequestTest(REST_XML, "HttpEmptyPrefixHeadersRequestClient"),
                FailingTest.RequestTest(REST_JSON, "RestJsonHttpEmptyPrefixHeadersRequestClient"),
            )

        private val BrokenTests:
            Set<BrokenTest> = setOf()
    }

    override val appliesTo: AppliesTo
        get() = AppliesTo.CLIENT
    override val expectFail: Set<FailingTest>
        get() = ExpectFail
    override val generateOnly: Set<String>
        get() = emptySet()
    override val disabledTests: Set<String>
        get() = emptySet()
    override val brokenTests: Set<BrokenTest>
        get() = BrokenTests

    override val logger: Logger = Logger.getLogger(javaClass.name)

    private val rc = codegenContext.runtimeConfig

    private val inputShape = operationShape.inputShape(codegenContext.model)
    private val outputShape = operationShape.outputShape(codegenContext.model)

    private val instantiator = ClientInstantiator(codegenContext, withinTest = true)

    private val codegenScope =
        arrayOf(
            "AssertEq" to RT.PrettyAssertions.resolve("assert_eq!"),
            "Uri" to RT.Http.resolve("Uri"),
        )

    override fun RustWriter.renderAllTestCases(allTests: List<TestCase>) {
        for (it in allTests) {
            renderTestCaseBlock(it, this) {
                when (it) {
                    is TestCase.RequestTest -> this.renderHttpRequestTestCase(it.testCase)
                    is TestCase.ResponseTest -> this.renderHttpResponseTestCase(it.testCase, it.targetShape)
                    is TestCase.MalformedRequestTest -> PANIC("Client protocol test generation does not support HTTP compliance test case type `$it`")
                }
            }
        }
    }

    private fun RustWriter.renderHttpRequestTestCase(httpRequestTestCase: HttpRequestTestCase) {
        logger.info("Generating request test: ${httpRequestTestCase.id}")

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
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/4177):
        //  Until the incorrect separation is addressed, we need to rely on this workaround.
        val noAuthSchemeResolver =
            codegenContext.rootDecorator.authSchemeOptions(codegenContext, emptyList()).find {
                it.authSchemeId == SigV4Trait.ID
            }?.let { writable {} } ?: writable {
                // If the `Sigv4AuthDecorator` is absent in the codegen plugin, we add `noAuth` as a fallback
                // during protocol tests. This ensures compatibility when a test model references Sigv4,
                // but the codegen, built with the generic client plugin, does not include the decorator.
                rust(
                    ".auth_scheme_resolver(#T)",
                    AuthSchemeResolverGenerator(
                        codegenContext,
                        emptyList(),
                    ).noAuthSchemeResolver(),
                )
            }
        // support test cases that set the host value, e.g: https://github.com/smithy-lang/smithy/blob/be68f3bbdfe5bf50a104b387094d40c8069f16b1/smithy-aws-protocol-tests/model/restJson1/endpoint-paths.smithy#L19
        val host = "https://${httpRequestTestCase.host.orNull() ?: "example.com"}".dq()
        rustTemplate(
            """
            let (http_client, request_receiver) = #{capture_request}(None);
            let config_builder = #{config}::Config::builder()
                .with_test_defaults()
                #{no_auth_scheme_resolver:W}
                .endpoint_url($host);
            #{customParams}

            """,
            "capture_request" to
                CargoDependency.smithyHttpClientTestUtil(rc).toType()
                    .resolve("test_util::capture_request"),
            "config" to ClientRustModule.config,
            "customParams" to customParams,
            "no_auth_scheme_resolver" to noAuthSchemeResolver,
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

    private fun RustWriter.renderHttpResponseTestCase(
        testCase: HttpResponseTestCase,
        expectedShape: StructureShape,
    ) {
        logger.info("Generating response test: ${testCase.id}")

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
        val mediaType = testCase.bodyMediaType.orNull()
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
                    #{SdkBody}::from(#{copy_from_slice}(&#{decode_body_data}(body.bytes().unwrap(), #{MediaType}::from(${(mediaType ?: "unknown").dq()}))))
                });
                de.deserialize_nonstreaming(&http_response)
            });
            """,
            "copy_from_slice" to RT.Bytes.resolve("copy_from_slice"),
            "decode_body_data" to RT.protocolTest(rc, "decode_body_data"),
            "DeserializeResponse" to RT.smithyRuntimeApiClient(rc).resolve("client::ser_de::DeserializeResponse"),
            "MediaType" to RT.protocolTest(rc, "MediaType"),
            "Operation" to codegenContext.symbolProvider.toSymbol(operationShape),
            "RuntimePlugin" to RT.runtimePlugin(rc),
            "SdkBody" to RT.sdkBody(rc),
            "SharedResponseDeserializer" to
                RT.smithyRuntimeApiClient(rc)
                    .resolve("client::ser_de::SharedResponseDeserializer"),
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
                // No body.
                #{AssertEq}(&body, &bytes::Bytes::new());
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
}
