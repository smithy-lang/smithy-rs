/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.letIf

class AwsQueryCompatibleTest {
    companion object {
        const val prologue = """
            namespace test
            use aws.protocols#awsJson1_0
            use aws.protocols#awsQueryCompatible
            use aws.protocols#awsQueryError
        """

        const val awsjson10Trait = "@awsJson1_0"
        const val awsQueryCompatibleTrait = "@awsQueryCompatible"

        fun testService(withAwsQueryError: Boolean = true) =
            """
            service TestService {
                version: "2023-02-20",
                operations: [SomeOperation]
            }

            operation SomeOperation {
                input: SomeOperationInputOutput,
                output: SomeOperationInputOutput,
                errors: [InvalidThingException],
            }

            structure SomeOperationInputOutput {
                a: String,
                b: Integer
            }
            """.letIf(withAwsQueryError) {
                it +
                    """
                    @awsQueryError(
                        code: "InvalidThing",
                        httpResponseCode: 400,
                    )
                    """
            }.let {
                it +
                    """
                    @error("client")
                    structure InvalidThingException {
                            message: String
                    }
                    """
            }
    }

    @Test
    fun `aws-query-compatible json with aws query error should allow for retrieving error code and type from custom header`() {
        val model =
            (prologue + awsQueryCompatibleTrait + awsjson10Trait + testService()).asSmithyModel(
                smithyVersion = "2",
            )
        clientIntegrationTest(model) { context, rustCrate ->
            rustCrate.testModule {
                tokioTest("should_parse_code_and_type_fields") {
                    rustTemplate(
                        """
                        let response = |_: #{http_1x}::Request<#{SdkBody}>| {
                            #{http_1x}::Response::builder()
                                .header(
                                    "x-amzn-query-error",
                                    #{http_1x}::HeaderValue::from_static("AWS.SimpleQueueService.NonExistentQueue;Sender"),
                                )
                                .status(400)
                                .body(
                                    #{SdkBody}::from(
                                        r##"{
                                            "__type": "com.amazonaws.sqs##QueueDoesNotExist",
                                            "message": "Some user-visible message"
                                        }"##
                                    )
                                )
                                .unwrap()
                        };
                        let client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(#{infallible_client_fn}(response))
                                .endpoint_url("http://localhost:1234")
                                .build()
                        );
                        let error = dbg!(client.some_operation().send().await).err().unwrap().into_service_error();
                        assert_eq!(
                            #{Some}("AWS.SimpleQueueService.NonExistentQueue"),
                            error.meta().code(),
                        );
                        assert_eq!(#{Some}("Sender"), error.meta().extra("type"));
                        """,
                        *RuntimeType.preludeScope,
                        "SdkBody" to RuntimeType.sdkBody(context.runtimeConfig),
                        "infallible_client_fn" to
                            CargoDependency.smithyHttpClientTestUtil(context.runtimeConfig)
                                .toType().resolve("test_util::infallible_client_fn"),
                        "http_1x" to CargoDependency.Http1x.toType(),
                    )
                }
            }
        }
    }

    @Test
    fun `aws-query-compatible json without aws query error should allow for retrieving error code from payload`() {
        val model =
            (prologue + awsQueryCompatibleTrait + awsjson10Trait + testService(withAwsQueryError = false)).asSmithyModel(
                smithyVersion = "2",
            )
        clientIntegrationTest(model) { context, rustCrate ->
            rustCrate.testModule {
                tokioTest("should_parse_code_from_payload") {
                    rustTemplate(
                        """
                        let response = |_: #{http_1x}::Request<#{SdkBody}>| {
                            #{http_1x}::Response::builder()
                                .status(400)
                                .body(
                                    #{SdkBody}::from(
                                        r##"{
                                            "__type": "com.amazonaws.sqs##QueueDoesNotExist",
                                            "message": "Some user-visible message"
                                        }"##,
                                    )
                                )
                                .unwrap()
                        };
                        let client = crate::Client::from_conf(
                            crate::Config::builder()
                                .http_client(#{infallible_client_fn}(response))
                                .endpoint_url("http://localhost:1234")
                                .build()
                        );
                        let error = dbg!(client.some_operation().send().await).err().unwrap().into_service_error();
                        assert_eq!(#{Some}("QueueDoesNotExist"), error.meta().code());
                        assert_eq!(#{None}, error.meta().extra("type"));
                        """,
                        *RuntimeType.preludeScope,
                        "SdkBody" to RuntimeType.sdkBody(context.runtimeConfig),
                        "infallible_client_fn" to
                            CargoDependency.smithyHttpClientTestUtil(context.runtimeConfig)
                                .toType().resolve("test_util::infallible_client_fn"),
                        "http_1x" to CargoDependency.Http1x.toType(),
                    )
                }
            }
        }
    }

    @Test
    fun `request header should include x-amzn-query-mode when the service has the awsQueryCompatible trait`() {
        val model =
            (prologue + awsQueryCompatibleTrait + awsjson10Trait + testService()).asSmithyModel(
                smithyVersion = "2",
            )
        clientIntegrationTest(model) { context, rustCrate ->
            rustCrate.testModule {
                tokioTest("test_request_header_should_include_x_amzn_query_mode") {
                    rustTemplate(
                        """
                        let (http_client, rx) = #{capture_request}(#{None});
                        let config = crate::Config::builder()
                            .http_client(http_client)
                            .endpoint_url("http://localhost:1234/SomeOperation")
                            .build();
                        let client = crate::Client::from_conf(config);
                        let _ = dbg!(client.some_operation().send().await);
                        let request = rx.expect_request();
                        assert_eq!("true", request.headers().get("x-amzn-query-mode").unwrap());
                        """,
                        *RuntimeType.preludeScope,
                        "capture_request" to RuntimeType.captureRequest(context.runtimeConfig),
                    )
                }
            }
        }
    }
}
