package software.amazon.smithy.rustsdk

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

internal class HttpChecksumTest {
    companion object {
        private const val PREFIX = "\$version: \"2\""
        private val model =
            """
            $PREFIX
            namespace test

            use aws.api#service
            use aws.auth#sigv4
            use aws.protocols#httpChecksum
            use aws.protocols#restJson1
            use smithy.rules#endpointRuleSet

            @service(sdkId: "dontcare")
            @restJson1
            @sigv4(name: "dontcare")
            @auth([sigv4])
            @endpointRuleSet({
                "version": "1.0",
                "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
                "parameters": {
                    "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
                }
            })
            service TestService {
                version: "2023-01-01",
                operations: [SomeOperation, SomeStreamingOperation]
            }

            @http(uri: "/SomeOperation", method: "POST")
            @optionalAuth
            @httpChecksum(
                requestChecksumRequired: true,
                requestAlgorithmMember: "checksumAlgorithm",
                requestValidationModeMember: "validationMode",
                responseAlgorithms: ["CRC32", "CRC32C", "SHA1", "SHA256"]
            )
            operation SomeOperation {
                input: SomeInput,
                output: SomeOutput
            }

            @input
            structure SomeInput {
                @httpHeader("x-amz-request-algorithm")
                checksumAlgorithm: ChecksumAlgorithm

                @httpHeader("x-amz-response-validation-mode")
                validationMode: ValidationMode

                @httpPayload
                @required
                body: Blob
            }

            @output
            structure SomeOutput {}

            @http(uri: "/SomeStreamingOperation", method: "POST")
            @optionalAuth
            @httpChecksum(
                requestChecksumRequired: true,
                requestAlgorithmMember: "checksumAlgorithm",
                requestValidationModeMember: "validationMode",
                responseAlgorithms: ["CRC32", "CRC32C", "SHA1", "SHA256"]
            )
            operation SomeStreamingOperation {
                input: SomeStreamingInput,
                output: SomeStreamingOutput
            }

            @streaming
            blob StreamingBlob

            @input
            structure SomeStreamingInput {
                @httpHeader("x-amz-request-algorithm")
                checksumAlgorithm: ChecksumAlgorithm

                @httpHeader("x-amz-response-validation-mode")
                validationMode: ValidationMode

                @httpPayload
                @required
                body: StreamingBlob
            }

            @output
            structure SomeStreamingOutput {}

            enum ChecksumAlgorithm {
                CRC32
                CRC32C
                //CRC64NVME
                SHA1
                SHA256
            }

            enum ValidationMode {
                ENABLED
            }
            """.asSmithyModel()
    }

    private val codegenContext = testClientCodegenContext(model)
//    private val symbolProvider = codegenContext.symbolProvider
//    private val operationShape = model.lookup<ServiceShape>("com.test#TestService")

    // TODO(flexibleChecksums): We can remove the explicit setting of .checksum_algorithm here when modeled defaults for
    // enums work.
    @Test
    fun requestChecksumWorks() {
        awsSdkIntegrationTest(model) { context, rustCrate ->

            val rc = context.runtimeConfig
            val moduleName = context.moduleUseName()
            rustCrate.integrationTest("request_checksums") {
                rustTemplate(
                    """
                    ##![cfg(feature = "test-util")]
                    ##![allow(unused_imports)]

                    use #{ByteStream};
                    use #{Blob};
                    use #{Region};
                    use #{pretty_assertions}::{assert_eq, assert_ne};
                    use #{Sigv4};
                    use #{tempfile};

                    ##[#{tokio}::test]
                    async fn crc32_checksums_work() {
                        let (http_client, rx) = ::aws_smithy_runtime::client::http::test_util::capture_request(None);
                        let config = $moduleName::Config::builder()
                            .region(Region::from_static("doesntmatter"))
                            .with_test_defaults()
                            .http_client(http_client)
                            .build();

                        let client = $moduleName::Client::from_conf(config);
                        let _ = client.some_operation()
                        .body(Blob::new(b"Hello world"))
                        .checksum_algorithm($moduleName::types::ChecksumAlgorithm::Crc32)
                        .send()
                        .await;
                        let request = rx.expect_request();
                        let crc32_header = request.headers()
                            .get("x-amz-checksum-crc32")
                            .expect("crc32 header should exist");

                        assert_eq!(crc32_header, "i9aeUg==");

                        let algo_header = request.headers()
                            .get("x-amz-request-algorithm")
                            .expect("algo header should exist");

                        assert_eq!(algo_header, "CRC32");
                    }
                    """,
                    *preludeScope,
                    "ByteStream" to RuntimeType.smithyTypes(rc).resolve("byte_stream::ByteStream"),
                    "Blob" to RuntimeType.smithyTypes(rc).resolve("Blob"),
                    "Region" to AwsRuntimeType.awsTypes(rc).resolve("region::Region"),
                    "tokio" to CargoDependency.Tokio.toType(),
                    "capture_request" to RuntimeType.captureRequest(rc),
                    "pretty_assertions" to CargoDependency.PrettyAssertions.toType(),
                    "tempfile" to CargoDependency.TempFile.copy(scope = DependencyScope.Compile).toType(),
                    "Length" to RuntimeType.smithyTypes(rc).resolve("byte_stream::Length"),
                    "Sigv4" to AwsCargoDependency.awsSigv4(rc).copy(scope = DependencyScope.Build).toType(),
//                    "AwsConfig" to AwsCargoDependency.awsConfig(rc).toType(),
                )
            }
        }
    }

//    @Test
//    fun `generate unnamed enums`() {
//        val shape = model.lookup<StringShape>("com.test#UnnamedEnum")
//        val sut = ClientInstantiator(codegenContext)
//        val data = Node.parse("t2.nano".dq())
//
//        val project = TestWorkspace.testProject(symbolProvider)
//        project.moduleFor(shape) {
//            ClientEnumGenerator(codegenContext, shape).render(this)
//            unitTest("generate_unnamed_enums") {
//                withBlock("let result = ", ";") {
//                    sut.render(this, shape, data)
//                }
//                rust("""assert_eq!(result, UnnamedEnum("t2.nano".to_owned()));""")
//            }
//        }
//        project.compileAndTest()
//    }
}

//
// internal class ChecksumTestGenerator(
//    private val testCases: List<EndpointTestCase>,
//    private val paramsType: RuntimeType,
//    private val resolverType: RuntimeType,
//    private val params: Parameters,
//    codegenContext: ClientCodegenContext,
// ) {
//    private val runtimeConfig = codegenContext.runtimeConfig
//    private val types = Types(runtimeConfig)
//    private val codegenScope =
//        arrayOf(
//            "Endpoint" to types.smithyEndpoint,
//            "Error" to types.resolveEndpointError,
//            "Document" to RuntimeType.document(runtimeConfig),
//            "HashMap" to RuntimeType.HashMap,
//            "capture_request" to RuntimeType.captureRequest(runtimeConfig),
//        )
//
//    private val instantiator = ClientInstantiator(codegenContext)
//
//    private fun EndpointTestCase.docs(): Writable {
//        val self = this
//        return writable { docs(self.documentation.orElse("no docs")) }
//    }
//
//    private fun generateBaseTest(
//        testCase: EndpointTestCase,
//        id: Int,
//    ): Writable =
//        writable {
//            rustTemplate(
//                """
//                #{docs:W}
//                ##[test]
//                fn test_$id() {
//                    let params = #{params:W};
//                    let resolver = #{resolver}::new();
//                    let endpoint = resolver.resolve_endpoint(&params);
//                    #{assertion:W}
//                }
//                """,
//                *codegenScope,
//                "docs" to testCase.docs(),
//                "params" to params(testCase),
//                "resolver" to resolverType,
//                "assertion" to
//                    writable {
//                        testCase.expect.endpoint.ifPresent { endpoint ->
//                            rustTemplate(
//                                """
//                                let endpoint = endpoint.expect("Expected valid endpoint: ${escape(endpoint.url)}");
//                                assert_eq!(endpoint, #{expected:W});
//                                """,
//                                *codegenScope, "expected" to generateEndpoint(endpoint),
//                            )
//                        }
//                        testCase.expect.error.ifPresent { error ->
//                            val expectedError =
//                                escape("expected error: $error [${testCase.documentation.orNull() ?: "no docs"}]")
//                            rustTemplate(
//                                """
//                                let error = endpoint.expect_err(${expectedError.dq()});
//                                assert_eq!(format!("{}", error), ${escape(error).dq()})
//                                """,
//                                *codegenScope,
//                            )
//                        }
//                    },
//            )
//        }
//
//    fun generate(): Writable =
//        writable {
//            var id = 0
//            testCases.forEach { testCase ->
//                id += 1
//                generateBaseTest(testCase, id)(this)
//            }
//        }
//
//    private fun params(testCase: EndpointTestCase) =
//        writable {
//            rust("#T::builder()", paramsType)
//            testCase.params.members.forEach { (id, value) ->
//                if (params.get(Identifier.of(id)).isPresent) {
//                    rust(".${Identifier.of(id).rustName()}(#W)", generateValue(Value.fromNode(value)))
//                }
//            }
//            rust(""".build().expect("invalid params")""")
//        }
//
//    private fun generateValue(value: Value): Writable {
//        return {
//            when (value) {
//                is StringValue -> rust(escape(value.value).dq() + ".to_string()")
//                is BooleanValue -> rust(value.toString())
//                is ArrayValue -> {
//                    rust(
//                        "vec![#W]",
//                        value.values.map { member ->
//                            writable {
//                                rustTemplate(
//                                    "#{Document}::from(#{value:W})",
//                                    *codegenScope,
//                                    "value" to generateValue(member),
//                                )
//                            }
//                        }.join(","),
//                    )
//                }
//
//                is IntegerValue -> rust(value.value.toString())
//
//                is RecordValue ->
//                    rustBlock("") {
//                        rustTemplate(
//                            "let mut out = #{HashMap}::<String, #{Document}>::new();",
//                            *codegenScope,
//                        )
//                        val ids = mutableListOf<Identifier>()
//                        value.value.forEach { (id, _) -> ids.add(id) }
//                        ids.forEach { id ->
//                            val v = value.get(id)
//                            rust(
//                                "out.insert(${id.toString().dq()}.to_string(), #W.into());",
//                                // When writing into the hashmap, it always needs to be an owned type
//                                generateValue(v),
//                            )
//                        }
//                        rustTemplate("out")
//                    }
//
//                else -> PANIC("unexpected type: $value")
//            }
//        }
//    }
//
//    private fun generateEndpoint(value: ExpectedEndpoint) =
//        writable {
//            rustTemplate("#{Endpoint}::builder().url(${escape(value.url).dq()})", *codegenScope)
//            value.headers.forEach { (headerName, values) ->
//                values.forEach { headerValue ->
//                    rust(".header(${headerName.dq()}, ${headerValue.dq()})")
//                }
//            }
//            value.properties.forEach { (name, value) ->
//                rust(
//                    ".property(${name.dq()}, #W)",
//                    generateValue(Value.fromNode(value)),
//                )
//            }
//            rust(".build()")
//        }
// }
//
//
