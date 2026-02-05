/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.plus
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.util.dq

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
                operations: [HttpChecksumOperation, HttpChecksumStreamingOperation]
            }

            @http(uri: "/HttpChecksumOperation", method: "POST")
            @optionalAuth
            @httpChecksum(
                requestChecksumRequired: true,
                requestAlgorithmMember: "checksumAlgorithm",
                requestValidationModeMember: "validationMode",
                responseAlgorithms: ["CRC32", "CRC32C", "CRC64NVME", "SHA1", "SHA256"]
            )
            operation HttpChecksumOperation {
                input: SomeInput,
                output: SomeOutput
            }

            @input
            structure SomeInput {
                @httpHeader("x-amz-request-algorithm")
                checksumAlgorithm: ChecksumAlgorithm

                @httpHeader("x-amz-response-validation-mode")
                validationMode: ValidationMode

                @httpHeader("x-amz-checksum-crc32")
                ChecksumCRC32: String

                @httpHeader("x-amz-checksum-crc32c")
                ChecksumCRC32C: String

                @httpHeader("x-amz-checksum-crc64nvme")
                ChecksumCRC64Nvme: String

                @httpHeader("x-amz-checksum-sha1")
                ChecksumSHA1: String

                @httpHeader("x-amz-checksum-sha256")
                ChecksumSHA256: String

                @httpHeader("x-amz-checksum-foo")
                ChecksumFoo: String

                @httpPayload
                @required
                body: Blob
            }

            @output
            structure SomeOutput {}

            @http(uri: "/HttpChecksumStreamingOperation", method: "POST")
            @optionalAuth
            @httpChecksum(
                requestChecksumRequired: true,
                requestAlgorithmMember: "checksumAlgorithm",
                requestValidationModeMember: "validationMode",
                responseAlgorithms: ["CRC32", "CRC32C", "CRC64NVME", "SHA1", "SHA256"]
            )
            operation HttpChecksumStreamingOperation {
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
                CRC64NVME
                SHA1
                SHA256
            }

            enum ValidationMode {
                ENABLED
            }
            """.asSmithyModel()
    }

    @Test
    fun requestChecksumWorks() {
        awsSdkIntegrationTest(model) { context, rustCrate ->
            // Allows us to use the user-agent test-utils in aws-runtime
            rustCrate.mergeFeature(Feature("test-util", true, listOf("aws-runtime/test-util")))
            val rc = context.runtimeConfig

            // Create Writables for all test types
            val checksumRequestTestWritables =
                checksumRequestTests.map { createRequestChecksumCalculationTest(it, context) }.join("\n")
            val checksumResponseSuccTestWritables =
                checksumResponseSuccTests.map { createResponseChecksumValidationSuccessTest(it, context) }.join("\n")
            val checksumResponseFailTestWritables =
                checksumResponseFailTests.map { createResponseChecksumValidationFailureTest(it, context) }.join("\n")
            val checksumStreamingRequestTestWritables =
                streamingRequestTests.map { createStreamingRequestChecksumCalculationTest(it, context) }.join("\n")
            val userProvidedChecksumTestWritables =
                userProvidedChecksumTests.map { createUserProvidedChecksumsTest(it, context) }.join("\n")
            val miscTests = createMiscellaneousTests(context)

            // Shared imports for all test types
            // Note about the `//#{PresigningMarker}` below. The `RequestChecksumInterceptor` relies on the `PresigningMarker` type from
            // the presigning inlineable. The decorator for that inlineable doesn't play nicely with the test model from the SEP, so we
            // use this as a kind of blunt way to include the presigning inlineable without actually wiring it up with the model.
            // We also have to ensure the `http-1x` feature the presigning inlineable expects is present on the generated crate.
            rustCrate.mergeFeature(
                Feature(
                    "http-1x",
                    default = false,
                    listOf("aws-smithy-runtime-api/http-1x"),
                ),
            )

            val testBase =
                writable {
                    rustTemplate(
                        """
                        ##![cfg(feature = "test-util")]
                        ##![allow(unused_imports)]

                        use #{Blob};
                        use #{Region};
                        use #{pretty_assertions}::assert_eq;
                        use #{SdkBody};
                        use std::io::Write;
                        use http_body_1x::Body;
                        use http_body_util::BodyExt;
                        use #{HttpRequest};
                        use #{UaAssert};
                        use #{UaExtract};
                        //#{PresigningMarker};
                        """,
                        *preludeScope,
                        "Blob" to RuntimeType.smithyTypes(rc).resolve("Blob"),
                        "Region" to AwsRuntimeType.awsTypes(rc).resolve("region::Region"),
                        "pretty_assertions" to CargoDependency.PrettyAssertions.toType(),
                        "SdkBody" to RuntimeType.smithyTypes(rc).resolve("body::SdkBody"),
                        "HttpRequest" to RuntimeType.smithyRuntimeApi(rc).resolve("client::orchestrator::HttpRequest"),
                        "UaAssert" to
                            AwsRuntimeType.awsRuntime(rc)
                                .resolve("user_agent::test_util::assert_ua_contains_metric_values"),
                        "UaExtract" to
                            AwsRuntimeType.awsRuntime(rc)
                                .resolve("user_agent::test_util::extract_ua_values"),
                        "PresigningMarker" to AwsRuntimeType.presigning().resolve("PresigningMarker"),
                    )
                }

            // Create one integ test per test type
            rustCrate.integrationTest("request_checksums") {
                testBase.plus(checksumRequestTestWritables)()
            }

            rustCrate.integrationTest("response_checksums_success") {
                testBase.plus(checksumResponseSuccTestWritables)()
            }

            rustCrate.integrationTest("response_checksums_fail") {
                testBase.plus(checksumResponseFailTestWritables)()
            }

            rustCrate.integrationTest("streaming_request_checksums") {
                testBase.plus(checksumStreamingRequestTestWritables)()
            }

            rustCrate.integrationTest("user_provided_checksums") {
                testBase.plus(userProvidedChecksumTestWritables)()
            }

            rustCrate.integrationTest("misc_tests") {
                testBase.plus(miscTests)()
            }
        }
    }

    /**
     * Generate tests where the request checksum is calculated correctly
     */
    private fun createRequestChecksumCalculationTest(
        testDef: RequestChecksumCalculationTest,
        context: ClientCodegenContext,
    ): Writable {
        val rc = context.runtimeConfig
        val moduleName = context.moduleUseName()
        val algoLower = testDef.checksumAlgorithm.lowercase()
        // If the algo is Crc32 don't explicitly set it to test that the default is correctly set
        val setChecksumAlgo =
            if (testDef.checksumAlgorithm != "Crc32") {
                ".checksum_algorithm($moduleName::types::ChecksumAlgorithm::${testDef.checksumAlgorithm})"
            } else {
                ""
            }
        return writable {
            rustTemplate(
                """
                //${testDef.docs}
                ##[#{tokio}::test]
                async fn ${algoLower}_request_checksums_work() {
                    let (http_client, rx) = #{capture_request}(None);
                    let config = $moduleName::Config::builder()
                        .region(Region::from_static("doesntmatter"))
                        .with_test_defaults()
                        .http_client(http_client)
                        .build();

                    let client = $moduleName::Client::from_conf(config);
                    let _ = client.http_checksum_operation()
                    .body(Blob::new(b"${testDef.requestPayload}"))
                    $setChecksumAlgo
                    .send()
                    .await;
                    let request = rx.expect_request();
                    let ${algoLower}_header = request.headers()
                        .get("x-amz-checksum-$algoLower")
                        .expect("x-amz-checksum-$algoLower header should exist");

                    assert_eq!(${algoLower}_header, "${testDef.checksumHeader}");

                    let algo_header = request.headers()
                        .get("x-amz-request-algorithm")
                        .expect("algo header should exist");

                    assert_eq!(algo_header, "${testDef.algoHeader}");

                    // Check the user-agent metrics for the selected algo
                    assert_ua_contains_metric_values(
                        &request
                            .headers()
                            .get("x-amz-user-agent")
                            .expect("UA header should be present"),
                        &["${testDef.algoFeatureId}"],
                    );
                }
                """,
                *preludeScope,
                "tokio" to CargoDependency.Tokio.toType(),
                "capture_request" to RuntimeType.captureRequest(rc),
                "http_1x" to CargoDependency.Http1x.toType(),
            )
        }
    }

    /**
     * Generate tests where the request is streaming and checksum is calculated correctly
     */
    private fun createStreamingRequestChecksumCalculationTest(
        testDef: StreamingRequestChecksumCalculationTest,
        context: ClientCodegenContext,
    ): Writable {
        val rc = context.runtimeConfig
        val moduleName = context.moduleUseName()
        val algoLower = testDef.checksumAlgorithm.lowercase()
        // If the algo is Crc32 don't explicitly set it to test that the default is correctly set
        val setChecksumAlgo =
            if (testDef.checksumAlgorithm != "Crc32") {
                ".checksum_algorithm($moduleName::types::ChecksumAlgorithm::${testDef.checksumAlgorithm})"
            } else {
                ""
            }
        return writable {
            rustTemplate(
                """
                //${testDef.docs}
                ##[#{tokio}::test]
                async fn ${algoLower}_request_checksums_work() {
                    let (http_client, rx) = #{capture_request}(None);
                    let config = $moduleName::Config::builder()
                        .region(Region::from_static("doesntmatter"))
                        .with_test_defaults()
                        .http_client(http_client)
                        .build();

                    let client = $moduleName::Client::from_conf(config);

                    let mut file = tempfile::NamedTempFile::new().unwrap();
                    file.as_file_mut()
                    .write_all("${testDef.requestPayload}".as_bytes())
                    .unwrap();

                    let streaming_body = aws_smithy_types::byte_stream::ByteStream::read_from()
                        .path(&file)
                        .build()
                        .await
                        .unwrap();

                    let _operation = client
                        .http_checksum_streaming_operation()
                        .body(streaming_body)
                        $setChecksumAlgo
                        .send()
                        .await;

                    let request = rx.expect_request();

                    let headers = request.headers();

                    assert_eq!(
                        headers.get("x-amz-trailer").unwrap(),
                        "x-amz-checksum-$algoLower",
                    );
                    assert_eq!(headers.get("content-encoding").unwrap(), "aws-chunked");

                    let body = request
                    .body()
                    .try_clone()
                    .expect("body is retryable")
                    .collect()
                    .await
                    .expect("body is collectable");

                    let mut body_data = bytes::BytesMut::new();
                    body_data.extend_from_slice(&body.to_bytes());

                    let body_string = std::str::from_utf8(&body_data).unwrap();
                    assert!(body_string.contains("x-amz-checksum-$algoLower:${testDef.trailerChecksum}"));
                }
                """,
                *preludeScope,
                "tokio" to CargoDependency.Tokio.toType(),
                "capture_request" to RuntimeType.captureRequest(rc),
                "http_1x" to CargoDependency.Http1x.toType(),
            )
        }
    }

    /**
     * Generate tests where the response checksum validates successfully
     */
    private fun createResponseChecksumValidationSuccessTest(
        testDef: ResponseChecksumValidationSuccessTest,
        context: ClientCodegenContext,
    ): Writable {
        val rc = context.runtimeConfig
        val moduleName = context.moduleUseName()
        val algoLower = testDef.checksumAlgorithm.lowercase()
        return writable {
            rustTemplate(
                """
                //${testDef.docs}
                ##[::tokio::test]
                async fn ${algoLower}_response_checksums_works() {
                    let (http_client, _rx) = #{capture_request}(Some(
                        #{http_1x}::Response::builder()
                            .header("x-amz-checksum-$algoLower", "${testDef.checksumHeaderValue}")
                            .body(SdkBody::from("${testDef.responsePayload}"))
                            .unwrap(),
                    ));
                    let config = $moduleName::Config::builder()
                        .region(Region::from_static("doesntmatter"))
                        .with_test_defaults()
                        .http_client(http_client)
                        .build();

                    let client = $moduleName::Client::from_conf(config);
                    let res = client
                        .http_checksum_operation()
                        .body(Blob::new(b"Doesn't matter."))
                        .checksum_algorithm($moduleName::types::ChecksumAlgorithm::${testDef.checksumAlgorithm})
                        .validation_mode($moduleName::types::ValidationMode::Enabled)
                        .send()
                        .await;
                    assert!(res.is_ok())
                }
                """,
                *preludeScope,
                "tokio" to CargoDependency.Tokio.toType(),
                "capture_request" to RuntimeType.captureRequest(rc),
                "http_1x" to CargoDependency.Http1x.toType(),
            )
        }
    }

    /**
     * Generate tests where the response checksum fails to validate
     */
    private fun createResponseChecksumValidationFailureTest(
        testDef: ResponseChecksumValidationFailureTest,
        context: ClientCodegenContext,
    ): Writable {
        val rc = context.runtimeConfig
        val moduleName = context.moduleUseName()
        val algoLower = testDef.checksumAlgorithm.lowercase()
        return writable {
            rustTemplate(
                """
                //${testDef.docs}
                ##[::tokio::test]
                async fn ${algoLower}_response_checksums_fail_correctly() {
                    let (http_client, _rx) = #{capture_request}(Some(
                        #{http_1x}::Response::builder()
                            .header("x-amz-checksum-$algoLower", "${testDef.checksumHeaderValue}")
                            .body(SdkBody::from("${testDef.responsePayload}"))
                            .unwrap(),
                    ));
                    let config = $moduleName::Config::builder()
                        .region(Region::from_static("doesntmatter"))
                        .with_test_defaults()
                        .http_client(http_client)
                        .retry_config(#{RetryConfig}::disabled())
                        .build();

                    let client = $moduleName::Client::from_conf(config);
                    let res = client
                        .http_checksum_operation()
                        .body(Blob::new(b"Doesn't matter."))
                        .checksum_algorithm($moduleName::types::ChecksumAlgorithm::${testDef.checksumAlgorithm})
                        .validation_mode($moduleName::types::ValidationMode::Enabled)
                        .send()
                        .await;

                    assert!(res.is_err());

                    let boxed_err = res
                        .unwrap_err()
                        .into_source()
                        .unwrap()
                        .downcast::<aws_smithy_checksums::body::validate::Error>();
                    let typed_err = boxed_err.as_ref().unwrap().as_ref();

                    match typed_err {
                        aws_smithy_checksums::body::validate::Error::ChecksumMismatch { actual, .. } => {
                            let calculated_checksum = aws_smithy_types::base64::encode(actual);
                            assert_eq!(calculated_checksum, "${testDef.calculatedChecksum}");
                        }
                        _ => panic!("Unknown error type in checksum validation"),
                    };
                }
                """,
                *preludeScope,
                "tokio" to CargoDependency.Tokio.toType(),
                "capture_request" to RuntimeType.captureRequest(rc),
                "http_1x" to CargoDependency.Http1x.toType(),
                "RetryConfig" to RuntimeType.smithyTypes(rc).resolve("retry::RetryConfig"),
            )
        }
    }

    /**
     * Generate tests for the case where a user provides a checksum
     */
    private fun createUserProvidedChecksumsTest(
        testDef: UserProvidedChecksumTest,
        context: ClientCodegenContext,
    ): Writable {
        val rc = context.runtimeConfig
        val moduleName = context.moduleUseName()
        val algoLower = testDef.checksumAlgorithm.lowercase()
        // We treat the c after crc32c and the nvme after crc64nvme as separate words
        // so this quick map helps us find the field to set
        val algoFieldNames =
            mapOf(
                "crc32" to "checksum_crc32",
                "crc32c" to "checksum_crc32_c",
                "crc64nvme" to "checksum_crc64_nvme",
                "foo" to "checksum_foo",
                "sha1" to "checksum_sha1",
                "sha256" to "checksum_sha256",
            )

        return writable {
            rustTemplate(
                """
                //${testDef.docs}
                ##[#{tokio}::test]
                async fn user_provided_${algoLower}_request_checksum_works() {
                    let (http_client, rx) = #{capture_request}(None);
                    let config = $moduleName::Config::builder()
                        .region(Region::from_static("doesntmatter"))
                        .with_test_defaults()
                        .http_client(http_client)
                        .build();

                    let client = $moduleName::Client::from_conf(config);
                    let _ = client.http_checksum_operation()
                    .body(Blob::new(b"${testDef.requestPayload}"))
                    .${algoFieldNames.get(algoLower)}(${testDef.checksumValue.dq()})
                    .send()
                    .await;
                    let request = rx.expect_request();
                    let ${algoLower}_header = request.headers()
                        .get("x-amz-checksum-$algoLower")
                        .expect("x-amz-checksum-$algoLower header should exist");

                    assert_eq!(${algoLower}_header, "${testDef.expectedHeaderValue}");

                    let algo_header = request.headers()
                        .get("x-amz-request-algorithm");

                    assert!(algo_header.is_none());
                }
                """,
                *preludeScope,
                "tokio" to CargoDependency.Tokio.toType(),
                "capture_request" to RuntimeType.captureRequest(rc),
                "http_1x" to CargoDependency.Http1x.toType(),
            )
        }
    }

    /**
     * Generate miscellaneous tests, currently mostly focused on the inclusion of the checksum config metrics in the
     * user-agent header
     */
    private fun createMiscellaneousTests(context: ClientCodegenContext): Writable {
        val rc = context.runtimeConfig
        val moduleName = context.moduleUseName()
        return writable {
            rustTemplate(
                """
                // The following tests confirm that the user-agent business metrics header is correctly
                // set for the request/response checksum config values
                ##[::tokio::test]
                async fn request_config_ua_required() {
                    let (http_client, rx) = #{capture_request}(None);
                    let config = $moduleName::Config::builder()
                        .region(Region::from_static("doesntmatter"))
                        .with_test_defaults()
                        .http_client(http_client)
                        .request_checksum_calculation(
                            aws_types::sdk_config::RequestChecksumCalculation::WhenRequired,
                        )
                        .build();

                    let client = $moduleName::Client::from_conf(config);
                    let _ = client
                        .http_checksum_operation()
                        .body(Blob::new(b"Doesn't matter"))
                        .send()
                        .await;
                    let request = rx.expect_request();

                    let sdk_metrics = extract_ua_values(
                        &request
                            .headers()
                            .get("x-amz-user-agent")
                            .expect("UA header should be present"),
                    ).expect("UA header should be present");
                    assert!(sdk_metrics.contains(&"a"));
                    assert!(!sdk_metrics.contains(&"Z"));
                }

                ##[::tokio::test]
                async fn request_config_ua_supported() {
                    let (http_client, rx) = #{capture_request}(None);
                    let config = $moduleName::Config::builder()
                        .region(Region::from_static("doesntmatter"))
                        .with_test_defaults()
                        .http_client(http_client)
                        .build();

                    let client = $moduleName::Client::from_conf(config);
                    let _ = client
                        .http_checksum_operation()
                        .body(Blob::new(b"Doesn't matter"))
                        .send()
                        .await;
                    let request = rx.expect_request();

                    let sdk_metrics = extract_ua_values(
                        &request
                            .headers()
                            .get("x-amz-user-agent")
                            .expect("UA header should be present"),
                    ).expect("UA header should be present");
                    assert!(sdk_metrics.contains(&"Z"));
                    assert!(!sdk_metrics.contains(&"a"));
                }

                ##[::tokio::test]
                async fn response_config_ua_supported() {
                    let (http_client, rx) = #{capture_request}(Some(
                        #{http_1x}::Response::builder()
                            .header("x-amz-checksum-crc32", "i9aeUg==")
                            .body(SdkBody::from("Hello world"))
                            .unwrap(),
                    ));
                    let config = $moduleName::Config::builder()
                        .region(Region::from_static("doesntmatter"))
                        .with_test_defaults()
                        .http_client(http_client)
                        .build();

                    let client = $moduleName::Client::from_conf(config);
                    let _ = client
                        .http_checksum_operation()
                        .body(Blob::new(b"Doesn't matter"))
                        .send()
                        .await;
                    let request = rx.expect_request();

                    let sdk_metrics = extract_ua_values(
                        &request
                            .headers()
                            .get("x-amz-user-agent")
                            .expect("UA header should be present"),
                    ).expect("UA header should be present");
                    assert!(sdk_metrics.contains(&"b"));
                    assert!(!sdk_metrics.contains(&"c"));
                }

                ##[::tokio::test]
                async fn response_config_ua_required() {
                    let (http_client, rx) = #{capture_request}(Some(
                        #{http_1x}::Response::builder()
                            .header("x-amz-checksum-crc32", "i9aeUg==")
                            .body(SdkBody::from("Hello world"))
                            .unwrap(),
                    ));
                    let config = $moduleName::Config::builder()
                        .region(Region::from_static("doesntmatter"))
                        .with_test_defaults()
                        .http_client(http_client)
                        .response_checksum_validation(
                            aws_types::sdk_config::ResponseChecksumValidation::WhenRequired,
                        )
                        .build();

                    let client = $moduleName::Client::from_conf(config);
                    let _ = client
                        .http_checksum_operation()
                        .body(Blob::new(b"Doesn't matter"))
                        .send()
                        .await;
                    let request = rx.expect_request();

                    let sdk_metrics = extract_ua_values(
                        &request
                            .headers()
                            .get("x-amz-user-agent")
                            .expect("UA header should be present"),
                    ).expect("UA header should be present");
                    assert!(sdk_metrics.contains(&"c"));
                    assert!(!sdk_metrics.contains(&"b"));
                }
                """,
                *preludeScope,
                "tokio" to CargoDependency.Tokio.toType(),
                "capture_request" to RuntimeType.captureRequest(rc),
                "http_1x" to CargoDependency.Http1x.toType(),
            )
        }
    }
}

// Classes and data for test definitions

data class RequestChecksumCalculationTest(
    val docs: String,
    val requestPayload: String,
    val checksumAlgorithm: String,
    val algoHeader: String,
    val checksumHeader: String,
    val algoFeatureId: String,
)

val checksumRequestTests =
    listOf(
        RequestChecksumCalculationTest(
            "CRC32 checksum calculation works.",
            "Hello world",
            "Crc32",
            "CRC32",
            "i9aeUg==",
            "U",
        ),
        RequestChecksumCalculationTest(
            "CRC32C checksum calculation works.",
            "Hello world",
            "Crc32C",
            "CRC32C",
            "crUfeA==",
            "V",
        ),
        RequestChecksumCalculationTest(
            "CRC64NVME checksum calculation works.",
            "Hello world",
            "Crc64Nvme",
            "CRC64NVME",
            "OOJZ0D8xKts=",
            "W",
        ),
        RequestChecksumCalculationTest(
            "SHA1 checksum calculation works.",
            "Hello world",
            "Sha1",
            "SHA1",
            "e1AsOh9IyGCa4hLN+2Od7jlnP14=",
            "X",
        ),
        RequestChecksumCalculationTest(
            "SHA256 checksum calculation works.",
            "Hello world",
            "Sha256",
            "SHA256",
            "ZOyIygCyaOW6GjVnihtTFtIS9PNmskdyMlNKiuyjfzw=",
            "Y",
        ),
    )

data class StreamingRequestChecksumCalculationTest(
    val docs: String,
    val requestPayload: String,
    val checksumAlgorithm: String,
    val trailerChecksum: String,
)

val streamingRequestTests =
    listOf(
        StreamingRequestChecksumCalculationTest(
            "CRC32 streaming checksum calculation works.",
            "Hello world",
            "Crc32",
            "i9aeUg==",
        ),
        StreamingRequestChecksumCalculationTest(
            "CRC32C streaming checksum calculation works.",
            "Hello world",
            "Crc32C",
            "crUfeA==",
        ),
        StreamingRequestChecksumCalculationTest(
            "CRC64NVME streaming checksum calculation works.",
            "Hello world",
            "Crc64Nvme",
            "OOJZ0D8xKts=",
        ),
        StreamingRequestChecksumCalculationTest(
            "SHA1 streaming checksum calculation works.",
            "Hello world",
            "Sha1",
            "e1AsOh9IyGCa4hLN+2Od7jlnP14=",
        ),
        StreamingRequestChecksumCalculationTest(
            "SHA256 streaming checksum calculation works.",
            "Hello world",
            "Sha256",
            "ZOyIygCyaOW6GjVnihtTFtIS9PNmskdyMlNKiuyjfzw=",
        ),
    )

data class ResponseChecksumValidationSuccessTest(
    val docs: String,
    val responsePayload: String,
    val checksumAlgorithm: String,
    val checksumHeaderValue: String,
)

val checksumResponseSuccTests =
    listOf(
        ResponseChecksumValidationSuccessTest(
            "Successful payload validation with CRC32 checksum.",
            "Hello world",
            "Crc32",
            "i9aeUg==",
        ),
        ResponseChecksumValidationSuccessTest(
            "Successful payload validation with Crc32C checksum.",
            "Hello world",
            "Crc32C",
            "crUfeA==",
        ),
        ResponseChecksumValidationSuccessTest(
            "Successful payload validation with Crc64Nvme checksum.",
            "Hello world",
            "Crc64Nvme",
            "OOJZ0D8xKts=",
        ),
        ResponseChecksumValidationSuccessTest(
            "Successful payload validation with Sha1 checksum.",
            "Hello world",
            "Sha1",
            "e1AsOh9IyGCa4hLN+2Od7jlnP14=",
        ),
        ResponseChecksumValidationSuccessTest(
            "Successful payload validation with Sha256 checksum.",
            "Hello world",
            "Sha256",
            "ZOyIygCyaOW6GjVnihtTFtIS9PNmskdyMlNKiuyjfzw=",
        ),
    )

data class ResponseChecksumValidationFailureTest(
    val docs: String,
    val responsePayload: String,
    val checksumAlgorithm: String,
    val checksumHeaderValue: String,
    val calculatedChecksum: String,
)

val checksumResponseFailTests =
    listOf(
        ResponseChecksumValidationFailureTest(
            "Failed payload validation with CRC32 checksum.",
            "Hello world",
            "Crc32",
            "bm90LWEtY2hlY2tzdW0=",
            "i9aeUg==",
        ),
        ResponseChecksumValidationFailureTest(
            "Failed payload validation with CRC32C checksum.",
            "Hello world",
            "Crc32C",
            "bm90LWEtY2hlY2tzdW0=",
            "crUfeA==",
        ),
        ResponseChecksumValidationFailureTest(
            "Failed payload validation with CRC64NVME checksum.",
            "Hello world",
            "Crc64Nvme",
            "bm90LWEtY2hlY2tzdW0=",
            "OOJZ0D8xKts=",
        ),
        ResponseChecksumValidationFailureTest(
            "Failed payload validation with SHA1 checksum.",
            "Hello world",
            "Sha1",
            "bm90LWEtY2hlY2tzdW0=",
            "e1AsOh9IyGCa4hLN+2Od7jlnP14=",
        ),
        ResponseChecksumValidationFailureTest(
            "Failed payload validation with SHA256 checksum.",
            "Hello world",
            "Sha256",
            "bm90LWEtY2hlY2tzdW0=",
            "ZOyIygCyaOW6GjVnihtTFtIS9PNmskdyMlNKiuyjfzw=",
        ),
    )

data class UserProvidedChecksumTest(
    val docs: String,
    val requestPayload: String,
    val checksumAlgorithm: String,
    val checksumValue: String,
    val expectedHeaderName: String,
    val expectedHeaderValue: String,
    val forbidHeaderName: String,
)

val userProvidedChecksumTests =
    listOf(
        UserProvidedChecksumTest(
            "CRC32 checksum provided by user.",
            "Hello world",
            "Crc32",
            "i9aeUg==",
            "x-amz-checksum-crc32",
            "i9aeUg==",
            "x-amz-request-algorithm",
        ),
        UserProvidedChecksumTest(
            "CRC32C checksum provided by user.",
            "Hello world",
            "Crc32C",
            "crUfeA==",
            "x-amz-checksum-crc32c",
            "crUfeA==",
            "x-amz-request-algorithm",
        ),
        UserProvidedChecksumTest(
            "CRC64NVME checksum provided by user.",
            "Hello world",
            "Crc64Nvme",
            "OOJZ0D8xKts=",
            "x-amz-checksum-crc64nvme",
            "OOJZ0D8xKts=",
            "x-amz-request-algorithm",
        ),
        UserProvidedChecksumTest(
            "SHA1 checksum provided by user.",
            "Hello world",
            "Sha1",
            "e1AsOh9IyGCa4hLN+2Od7jlnP14=",
            "x-amz-checksum-sha1",
            "e1AsOh9IyGCa4hLN+2Od7jlnP14=",
            "x-amz-request-algorithm",
        ),
        UserProvidedChecksumTest(
            "SHA256 checksum provided by user.",
            "Hello world",
            "Sha256",
            "ZOyIygCyaOW6GjVnihtTFtIS9PNmskdyMlNKiuyjfzw=",
            "x-amz-checksum-sha256",
            "ZOyIygCyaOW6GjVnihtTFtIS9PNmskdyMlNKiuyjfzw=",
            "x-amz-request-algorithm",
        ),
        UserProvidedChecksumTest(
            "Forwards compatibility, unmodeled checksum provided by user.",
            "Hello world",
            "Foo",
            "This-is-not-a-real-checksum",
            "x-amz-checksum-foo",
            "This-is-not-a-real-checksum",
            "x-amz-request-algorithm",
        ),
    )
