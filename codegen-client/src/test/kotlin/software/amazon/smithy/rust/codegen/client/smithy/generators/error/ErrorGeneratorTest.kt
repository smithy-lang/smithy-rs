/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.error

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

class ErrorGeneratorTest {
    val model =
        """
        namespace com.test
        use aws.protocols#awsJson1_1

        @awsJson1_1
        service TestService {
            operations: [TestOp]
        }

        operation TestOp {
            errors: [MyError]
        }

        @error("server")
        @retryable
        structure MyError {
            message: String
        }
        """.asSmithyModel()

    @Test
    fun `generate error structure and builder`() {
        clientIntegrationTest(model) { _, rustCrate ->
            rustCrate.withFile("src/types/error.rs") {
                rust(
                    """
                    ##[test]
                    fn test_error_generator() {
                        use aws_smithy_types::error::metadata::{ErrorMetadata, ProvideErrorMetadata};
                        use aws_smithy_types::retry::ErrorKind;

                        let err = MyError::builder()
                            .meta(ErrorMetadata::builder().code("test").message("testmsg").build())
                            .message("testmsg")
                            .build();
                        assert_eq!(err.retryable_error_kind(), ErrorKind::ServerError);
                        assert_eq!("test", err.meta().code().unwrap());
                        assert_eq!("testmsg", err.meta().message().unwrap());
                        assert_eq!("testmsg", err.message().unwrap());
                    }
                    """,
                )
            }
        }
    }
}
