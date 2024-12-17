/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

class UserAgentDecoratorTest {
    companion object {
        // Can't use the dollar sign in a multiline string with doing it like this.
        private const val PREFIX = "\$version: \"2\""
        val model =
            """
            $PREFIX
            namespace test

            use aws.api#service
            use aws.auth#sigv4
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
                operations: [SomeOperation]
            }

            @http(uri: "/SomeOperation", method: "GET")
            @optionalAuth
            operation SomeOperation {
                input: SomeInput,
                output: SomeOutput
            }

            @input
            structure SomeInput {}

            @output
            structure SomeOutput {}
            """.asSmithyModel()
    }

    @Test
    fun smokeTestSdkCodegen() {
        awsSdkIntegrationTest(model) { _, _ ->
            // it should compile
        }
    }

    @Test
    fun userAgentWorks() {
        awsSdkIntegrationTest(model) { context, rustCrate ->
            val rc = context.runtimeConfig
            val moduleName = context.moduleUseName()
            rustCrate.integrationTest("user-agent") {
                rustTemplate(
                    """
                    use $moduleName::config::{AppName, Credentials, Region, SharedCredentialsProvider};
                    use $moduleName::{Config, Client};
                    use #{capture_request};

                    ##[#{tokio}::test]
                    async fn user_agent_app_name() {
                        let (http_client, rcvr) = capture_request(None);
                        let config = Config::builder()
                            .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .app_name(AppName::new("test-app-name").expect("valid app name")) // set app name in config
                            .build();
                        let client = Client::from_conf(config);
                        let _ = client.some_operation().send().await;

                        // verify app name made it to the user agent
                        let request = rcvr.expect_request();
                        let formatted = std::str::from_utf8(
                            request
                                .headers()
                                .get("x-amz-user-agent")
                                .unwrap()
                                .as_bytes(),
                        )
                        .unwrap();
                        assert!(
                            formatted.ends_with(" app/test-app-name"),
                            "'{}' didn't end with the app name",
                            formatted
                        );
                    }

                    ##[#{tokio}::test]
                    async fn user_agent_http_client() {
                        let (http_client, rcvr) = capture_request(None);
                        let config = Config::builder()
                            .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .app_name(AppName::new("test-app-name").expect("valid app name")) // set app name in config
                            .build();
                        let client = Client::from_conf(config);
                        let _ = client.some_operation().send().await;

                        // verify app name made it to the user agent
                        let request = rcvr.expect_request();
                        let formatted = std::str::from_utf8(
                            request
                                .headers()
                                .get("x-amz-user-agent")
                                .unwrap()
                                .as_bytes(),
                        )
                        .unwrap();
                        assert!(
                            formatted.contains("md/http##capture-request-handler"),
                            "'{}' didn't include connector metadata",
                            formatted
                        );
                    }
                    """,
                    *preludeScope,
                    "tokio" to CargoDependency.Tokio.toDevDependency().withFeature("rt").withFeature("macros").toType(),
                    "capture_request" to RuntimeType.captureRequest(rc),
                )
            }
        }
    }

    @Test
    fun `it emits business metric for RPC v2 CBOR in user agent`() {
        val model =
            """
            namespace test

            use aws.auth#sigv4
            use aws.api#service
            use smithy.protocols#rpcv2Cbor
            use smithy.rules#endpointRuleSet

            @auth([sigv4])
            @sigv4(name: "dontcare")
            @rpcv2Cbor
            @endpointRuleSet({
                "version": "1.0",
                "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
                "parameters": {}
            })
            @service(sdkId: "dontcare")
            service TestService { version: "2023-01-01", operations: [SomeOperation] }
            structure SomeOutput { something: String }
            operation SomeOperation { output: SomeOutput }
            """.asSmithyModel()

        awsSdkIntegrationTest(model) { ctx, rustCrate ->
            rustCrate.integrationTest("business_metric_for_rpc_v2_cbor") {
                tokioTest("should_emit_metric_in_user_agent") {
                    val rc = ctx.runtimeConfig
                    val moduleName = ctx.moduleUseName()
                    rustTemplate(
                        """
                        use $moduleName::config::Region;
                        use $moduleName::{Client, Config};

                        let (http_client, rcvr) = #{capture_request}(#{None});
                        let config = Config::builder()
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .with_test_defaults()
                            .build();
                        let client = Client::from_conf(config);
                        let _ = client.some_operation().send().await;
                        let expected_req = rcvr.expect_request();
                        let user_agent = expected_req
                            .headers()
                            .get("x-amz-user-agent")
                            .unwrap();
                        #{assert_ua_contains_metric_values}(user_agent, &["M"]);
                        """,
                        *preludeScope,
                        "assert_ua_contains_metric_values" to
                            AwsRuntimeType.awsRuntimeTestUtil(rc)
                                .resolve("user_agent::test_util::assert_ua_contains_metric_values"),
                        "capture_request" to RuntimeType.captureRequest(rc),
                        "disable_interceptor" to
                            RuntimeType.smithyRuntimeApiClient(rc)
                                .resolve("client::interceptors::disable_interceptor"),
                        "UserAgentInterceptor" to
                            AwsRuntimeType.awsRuntime(rc)
                                .resolve("user_agent::UserAgentInterceptor"),
                    )
                }
            }
        }
    }

    @Test
    fun `it emits business metric for checksum usage`() {
        val model =
            """
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
            """.asSmithyModel(smithyVersion = "2.0")

        awsSdkIntegrationTest(model) { ctx, rustCrate ->
            rustCrate.integrationTest("business_metrics_for_checksums") {
                val moduleName = ctx.moduleUseName()
                val rc = ctx.runtimeConfig
                rust(
                    """
                    use $moduleName::config::Region;
                    use $moduleName::types::ChecksumAlgorithm;
                    use $moduleName::primitives::Blob;
                    use $moduleName::{Client, Config};
                    """,
                )

                tokioTest("should_emit_metric_in_user_agent_for_crc32") {
                    rustTemplate(
                        """
                        let (http_client, rcvr) = #{capture_request}(#{None});
                        let config = Config::builder()
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .with_test_defaults()
                            .build();
                        let client = Client::from_conf(config);
                        let _ = client
                            .http_checksum_operation()
                            .checksum_algorithm(ChecksumAlgorithm::Crc32)
                            .body(Blob::new(b"Hello, world!"))
                            .send()
                            .await;
                        let expected_req = rcvr.expect_request();
                        let user_agent = expected_req
                            .headers()
                            .get("x-amz-user-agent")
                            .unwrap();
                        #{assert_ua_contains_metric_values}(user_agent, &["U"]);
                        """,
                        *preludeScope,
                        "assert_ua_contains_metric_values" to
                            AwsRuntimeType.awsRuntimeTestUtil(rc)
                                .resolve("user_agent::test_util::assert_ua_contains_metric_values"),
                        "capture_request" to RuntimeType.captureRequest(rc),
                        "disable_interceptor" to
                            RuntimeType.smithyRuntimeApiClient(rc)
                                .resolve("client::interceptors::disable_interceptor"),
                        "UserAgentInterceptor" to
                            AwsRuntimeType.awsRuntime(rc)
                                .resolve("user_agent::UserAgentInterceptor"),
                    )
                }

                tokioTest("should_emit_metric_in_user_agent_for_crc32c") {
                    rustTemplate(
                        """
                        let (http_client, rcvr) = #{capture_request}(#{None});
                        let config = Config::builder()
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .with_test_defaults()
                            .build();
                        let client = Client::from_conf(config);
                        let _ = client
                            .http_checksum_operation()
                            .checksum_algorithm(ChecksumAlgorithm::Crc32C)
                            .body(Blob::new(b"Hello, world!"))
                            .send()
                            .await;
                        let expected_req = rcvr.expect_request();
                        let user_agent = expected_req
                            .headers()
                            .get("x-amz-user-agent")
                            .unwrap();
                        #{assert_ua_contains_metric_values}(user_agent, &["V"]);
                        """,
                        *preludeScope,
                        "assert_ua_contains_metric_values" to
                            AwsRuntimeType.awsRuntimeTestUtil(rc)
                                .resolve("user_agent::test_util::assert_ua_contains_metric_values"),
                        "capture_request" to RuntimeType.captureRequest(rc),
                        "disable_interceptor" to
                            RuntimeType.smithyRuntimeApiClient(rc)
                                .resolve("client::interceptors::disable_interceptor"),
                        "UserAgentInterceptor" to
                            AwsRuntimeType.awsRuntime(rc)
                                .resolve("user_agent::UserAgentInterceptor"),
                    )
                }

                tokioTest("should_emit_metric_in_user_agent_for_sha1") {
                    rustTemplate(
                        """
                        let (http_client, rcvr) = #{capture_request}(#{None});
                        let config = Config::builder()
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .with_test_defaults()
                            .build();
                        let client = Client::from_conf(config);
                        let _ = client
                            .http_checksum_operation()
                            .checksum_algorithm(ChecksumAlgorithm::Sha1)
                            .body(Blob::new(b"Hello, world!"))
                            .send()
                            .await;
                        let expected_req = rcvr.expect_request();
                        let user_agent = expected_req
                            .headers()
                            .get("x-amz-user-agent")
                            .unwrap();
                        #{assert_ua_contains_metric_values}(user_agent, &["X"]);
                        """,
                        *preludeScope,
                        "assert_ua_contains_metric_values" to
                            AwsRuntimeType.awsRuntimeTestUtil(rc)
                                .resolve("user_agent::test_util::assert_ua_contains_metric_values"),
                        "capture_request" to RuntimeType.captureRequest(rc),
                        "disable_interceptor" to
                            RuntimeType.smithyRuntimeApiClient(rc)
                                .resolve("client::interceptors::disable_interceptor"),
                        "UserAgentInterceptor" to
                            AwsRuntimeType.awsRuntime(rc)
                                .resolve("user_agent::UserAgentInterceptor"),
                    )
                }

                tokioTest("should_emit_metric_in_user_agent_for_sha256") {
                    rustTemplate(
                        """
                        let (http_client, rcvr) = #{capture_request}(#{None});
                        let config = Config::builder()
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .with_test_defaults()
                            .build();
                        let client = Client::from_conf(config);
                        let _ = client
                            .http_checksum_operation()
                            .checksum_algorithm(ChecksumAlgorithm::Sha256)
                            .body(Blob::new(b"Hello, world!"))
                            .send()
                            .await;
                        let expected_req = rcvr.expect_request();
                        let user_agent = expected_req
                            .headers()
                            .get("x-amz-user-agent")
                            .unwrap();
                        #{assert_ua_contains_metric_values}(user_agent, &["Y"]);
                        """,
                        *preludeScope,
                        "assert_ua_contains_metric_values" to
                            AwsRuntimeType.awsRuntimeTestUtil(rc)
                                .resolve("user_agent::test_util::assert_ua_contains_metric_values"),
                        "capture_request" to RuntimeType.captureRequest(rc),
                        "disable_interceptor" to
                            RuntimeType.smithyRuntimeApiClient(rc)
                                .resolve("client::interceptors::disable_interceptor"),
                        "UserAgentInterceptor" to
                            AwsRuntimeType.awsRuntime(rc)
                                .resolve("user_agent::UserAgentInterceptor"),
                    )
                }
            }
        }
    }
}
