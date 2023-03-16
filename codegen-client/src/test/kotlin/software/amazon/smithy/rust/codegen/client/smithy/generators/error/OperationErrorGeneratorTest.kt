/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.error

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup

class OperationErrorGeneratorTest {
    private val model = """
        namespace error

        @aws.protocols#awsJson1_0
        service TestService {
            operations: [Greeting],
        }

        operation Greeting {
            errors: [InvalidGreeting, ComplexError, FooException, Deprecated]
        }

        @error("client")
        @retryable
        structure InvalidGreeting {
            message: String,
        }

        @error("server")
        structure FooException { }

        @error("server")
        structure ComplexError {
            abc: String,
            other: Integer
        }

        @error("server")
        @deprecated
        structure Deprecated { }
    """.asSmithyModel()

    @Test
    fun `generates combined error enums`() {
        clientIntegrationTest(model) { _, rustCrate ->
            rustCrate.moduleFor(model.lookup<StructureShape>("error#FooException")) {
                unitTest(
                    name = "generates_combined_error_enums",
                    test = """
                        use crate::operation::greeting::GreetingError;

                        let error = GreetingError::InvalidGreeting(
                            InvalidGreeting::builder()
                                .message("an error")
                                .meta(aws_smithy_types::Error::builder().code("InvalidGreeting").message("an error").build())
                                .build()
                        );
                        assert_eq!(format!("{}", error), "InvalidGreeting: an error");
                        assert_eq!(error.meta().message(), Some("an error"));
                        assert_eq!(error.meta().code(), Some("InvalidGreeting"));
                        use aws_smithy_types::retry::ProvideErrorKind;
                        assert_eq!(error.retryable_error_kind(), Some(aws_smithy_types::retry::ErrorKind::ClientError));

                        // Generate is_xyz methods for errors.
                        assert_eq!(error.is_invalid_greeting(), true);
                        assert_eq!(error.is_complex_error(), false);

                        // Unhandled variants properly delegate message.
                        let error = GreetingError::generic(aws_smithy_types::Error::builder().message("hello").build());
                        assert_eq!(error.meta().message(), Some("hello"));

                        let error = GreetingError::unhandled("some other error");
                        assert_eq!(error.meta().message(), None);
                        assert_eq!(error.meta().code(), None);

                        // Indicate the original name in the display output.
                        let error = FooError::builder().build();
                        assert_eq!(format!("{}", error), "FooError [FooException]");

                        let error = Deprecated::builder().build();
                        assert_eq!(error.to_string(), "Deprecated");
                    """,
                )
            }
        }
    }
}
