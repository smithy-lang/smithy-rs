/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.plus
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
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

//    private val codegenContext = testClientCodegenContext(model)
//    private val symbolProvider = codegenContext.symbolProvider
//    private val operationShape = model.lookup<ServiceShape>("com.test#TestService")

    // TODO(flexibleChecksums): We can remove the explicit setting of .checksum_algorithm for crc32 when modeled defaults
    //  for enums work.
    @Test
    fun requestChecksumWorks() {
        val checksumRequestTests =
            listOf(
                RequestChecksumCalculationTest(
                    "CRC32 checksum calculation works.",
                    "Hello world",
                    "Crc32",
                    "CRC32",
                    "i9aeUg==",
                ),
                RequestChecksumCalculationTest(
                    "CRC32C checksum calculation works.",
                    "Hello world",
                    "Crc32C",
                    "CRC32C",
                    "crUfeA==",
                ),
                /* We do not yet support Crc64Nvme checksums
                 RequestChecksumCalculationTest(
                 "CRC64NVME checksum calculation works.",
                 "Hello world",
                 "Crc64Nvme",
                 "CRC64NVME",
                 "uc8X9yrZrD4=",
                 ),
                 */
                RequestChecksumCalculationTest(
                    "SHA1 checksum calculation works.",
                    "Hello world",
                    "Sha1",
                    "SHA1",
                    "e1AsOh9IyGCa4hLN+2Od7jlnP14=",
                ),
                RequestChecksumCalculationTest(
                    "SHA256 checksum calculation works.",
                    "Hello world",
                    "Sha256",
                    "SHA256",
                    "ZOyIygCyaOW6GjVnihtTFtIS9PNmskdyMlNKiuyjfzw=",
                ),
            )

        awsSdkIntegrationTest(model) { context, rustCrate ->

            val rc = context.runtimeConfig
            val testWritables =
                checksumRequestTests.map { createRequestChecksumCalculationTest(it, context) }.join("\n")
            println("LNJ CREATING testBASE")
            val testBase =
                writable {
                    rustTemplate(
                        """
                        ##![cfg(feature = "test-util")]

                        use #{Blob};
                        use #{Region};
                        use #{pretty_assertions}::assert_eq;

                        """,
                        *preludeScope,
                        "Blob" to RuntimeType.smithyTypes(rc).resolve("Blob"),
                        "Region" to AwsRuntimeType.awsTypes(rc).resolve("region::Region"),
                        "pretty_assertions" to CargoDependency.PrettyAssertions.toType(),
                    )
                }
            rustCrate.integrationTest("request_checksums") {
                testBase.plus(testWritables)()
            }
        }
    }

    private fun createRequestChecksumCalculationTest(
        testDef: RequestChecksumCalculationTest,
        context: ClientCodegenContext,
    ): Writable {
        println("LNJ TEST")
        val rc = context.runtimeConfig
        val moduleName = context.moduleUseName()
        val algoLower = testDef.checksumAlgorithm.lowercase()
        return writable {
            rustTemplate(
                """
                //${testDef.docs}
                ##[#{tokio}::test]
                async fn ${algoLower}_checksums_work() {
                    let (http_client, rx) = #{capture_request}(None);
                    let config = $moduleName::Config::builder()
                        .region(Region::from_static("doesntmatter"))
                        .with_test_defaults()
                        .http_client(http_client)
                        .build();

                    let client = $moduleName::Client::from_conf(config);
                    let _ = client.some_operation()
                    .body(Blob::new(b"${testDef.requestPayload}"))
                    .checksum_algorithm($moduleName::types::ChecksumAlgorithm::${testDef.checksumAlgorithm})
                    .send()
                    .await;
                    let request = rx.expect_request();
                    let ${algoLower}_header = request.headers()
                        .get("x-amz-checksum-$algoLower")
                        .expect("$algoLower header should exist");

                    assert_eq!(${algoLower}_header, "${testDef.checksumHeader}");

                    let algo_header = request.headers()
                        .get("x-amz-request-algorithm")
                        .expect("algo header should exist");

                    assert_eq!(algo_header, "${testDef.algoHeader}");
                }
                """,
                *preludeScope,
                "tokio" to CargoDependency.Tokio.toType(),
                "capture_request" to RuntimeType.captureRequest(rc),
            )
        }
    }
}

data class RequestChecksumCalculationTest(
    val docs: String,
    val requestPayload: String,
    val checksumAlgorithm: String,
    val algoHeader: String,
    val checksumHeader: String,
)

data class StreamingRequestChecksumCalculationTest(
    val docs: String,
    val requestPayload: String,
    val checksumAlgorithm: String,
    val contentHeader: String,
    val trailerHeader: String,
    val trailerChecksum: String,
)

data class ResponseChecksumValidationTest(
    val docs: String,
    val responsePayload: String,
    val checksumAlgorithm: String,
    val algoHeader: String,
    val checksumHeader: String,
    val validationType: TestResult,
)

sealed class TestResult

object ResponseChecksumValidationTestSuccess : TestResult()

data class ResponseChecksumValidationTestFailure(val calculatedChecksum: String) : TestResult()
