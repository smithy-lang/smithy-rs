/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup

class AwsQueryCompatibleTest {
    @Test
    fun `aws-query-compatible json with aws query error should allow for retrieving error code and type from custom header`() {
        val model = """
            namespace test
            use aws.protocols#awsJson1_0
            use aws.protocols#awsQueryCompatible
            use aws.protocols#awsQueryError

            @awsQueryCompatible
            @awsJson1_0
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

            @awsQueryError(
                code: "InvalidThing",
                httpResponseCode: 400,
            )
            @error("client")
            structure InvalidThingException {
                message: String
            }
        """.asSmithyModel()

        clientIntegrationTest(model) { context, rustCrate ->
            val operation: OperationShape = context.model.lookup("test#SomeOperation")
            rustCrate.withModule(context.symbolProvider.moduleForShape(operation)) {
                rustTemplate(
                    """
                    ##[cfg(test)]
                    ##[#{tokio}::test]
                    async fn should_parse_code_and_type_fields() {
                        use aws_smithy_types::body::SdkBody;

                        let response = |_: http::Request<SdkBody>| {
                            http::Response::builder()
                                .header(
                                    "x-amzn-query-error",
                                    http::HeaderValue::from_static("AWS.SimpleQueueService.NonExistentQueue;Sender"),
                                )
                                .status(400)
                                .body(
                                    SdkBody::from(
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
                            Some("AWS.SimpleQueueService.NonExistentQueue"),
                            error.meta().code(),
                        );
                        assert_eq!(Some("Sender"), error.meta().extra("type"));
                    }
                    """,
                    "infallible_client_fn" to CargoDependency.smithyRuntimeTestUtil(context.runtimeConfig)
                        .toType().resolve("client::http::test_util::infallible_client_fn"),
                    "tokio" to CargoDependency.Tokio.toType(),
                )
            }
        }
    }

    @Test
    fun `aws-query-compatible json without aws query error should allow for retrieving error code from payload`() {
        val model = """
            namespace test
            use aws.protocols#awsJson1_0
            use aws.protocols#awsQueryCompatible

            @awsQueryCompatible
            @awsJson1_0
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

            @error("client")
            structure InvalidThingException {
                message: String
            }
        """.asSmithyModel()

        clientIntegrationTest(model) { context, rustCrate ->
            val operation: OperationShape = context.model.lookup("test#SomeOperation")
            rustCrate.withModule(context.symbolProvider.moduleForShape(operation)) {
                rustTemplate(
                    """
                    ##[cfg(test)]
                    ##[#{tokio}::test]
                    async fn should_parse_code_from_payload() {
                        use aws_smithy_types::body::SdkBody;

                        let response = |_: http::Request<SdkBody>| {
                            http::Response::builder()
                                .status(400)
                                .body(
                                    SdkBody::from(
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
                        assert_eq!(Some("QueueDoesNotExist"), error.meta().code());
                        assert_eq!(None, error.meta().extra("type"));
                    }
                    """,
                    "infallible_client_fn" to CargoDependency.smithyRuntimeTestUtil(context.runtimeConfig)
                        .toType().resolve("client::http::test_util::infallible_client_fn"),
                    "tokio" to CargoDependency.Tokio.toType(),
                )
            }
        }
    }
}
