/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

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

        clientIntegrationTest(model) { clientCodegenContext, rustCrate ->
            val moduleName = clientCodegenContext.moduleUseName()
            rustCrate.integrationTest("should_parse_code_and_type_fields") {
                rust(
                    """
                    ##[test]
                    fn should_parse_code_and_type_fields() {
                        use aws_smithy_http::response::ParseStrictResponse;

                        let response = http::Response::builder()
                            .header(
                                "x-amzn-query-error",
                                http::HeaderValue::from_static("AWS.SimpleQueueService.NonExistentQueue;Sender"),
                            )
                            .status(400)
                            .body(
                                r##"{
                                    "__type": "com.amazonaws.sqs##QueueDoesNotExist",
                                    "message": "Some user-visible message"
                                }"##,
                            )
                            .unwrap();
                        let some_operation = $moduleName::operation::SomeOperation::new();
                        let error = some_operation
                            .parse(&response.map(bytes::Bytes::from))
                            .err()
                            .unwrap();
                        assert_eq!(
                            error.meta().code(),
                            Some("AWS.SimpleQueueService.NonExistentQueue"),
                        );
                        assert_eq!(error.meta().extra("type"), Some("Sender"));
                    }
                    """,
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

        clientIntegrationTest(model) { clientCodegenContext, rustCrate ->
            val moduleName = clientCodegenContext.moduleUseName()
            rustCrate.integrationTest("should_parse_code_from_payload") {
                rust(
                    """
                    ##[test]
                    fn should_parse_code_from_payload() {
                        use aws_smithy_http::response::ParseStrictResponse;

                        let response = http::Response::builder()
                            .status(400)
                            .body(
                                r##"{
                                    "__type": "com.amazonaws.sqs##QueueDoesNotExist",
                                    "message": "Some user-visible message"
                                }"##,
                            )
                            .unwrap();
                        let some_operation = $moduleName::operation::SomeOperation::new();
                        let error = some_operation
                            .parse(&response.map(bytes::Bytes::from))
                            .err()
                            .unwrap();
                        assert_eq!(error.meta().code(), Some("QueueDoesNotExist"));
                        assert_eq!(error.meta().extra("type"), None);
                    }
                    """,
                )
            }
        }
    }
}
